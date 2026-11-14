//! ウィンドウを 1 つにまとめる（多重起動の抑止）。
//!
//! 設定を入れているとき、名前付きパイプを最初に作れたウィンドウが「まとめ先」になる。
//! 後から起動したものは、渡されたパスをまとめ先に送って、ウィンドウを出さずに終わる。
//!
//! - まとめ先を決めるのは `FILE_FLAG_FIRST_PIPE_INSTANCE`。同時に起動しても、パイプを最初に作れた 1 つだけがまとめ先になる。
//!   まとめ先は、つながってきたら次の受け口を先に作るので、動いている間はパイプが無くならない
//!   （「パイプが無い」と「受け口がふさがっている」を区別でき、まとめ先が 2 つできない）
//! - パイプの名前は、セッション、ユーザー、設定のファイルの場所ごとに変える。別のセッションの見えない画面や、
//!   設定の置き場所を変えて起動したもの（動作確認など）とはつながらない
//! - 送る側は、相手が同じユーザーのプロセスかを確かめてから送る。名前を先に取られても、パスを渡さない
//! - まとめ先が固まっている・閉じようとしているときは、待ちすぎずに自分のウィンドウで開く。
//!   まとめ先が開くか断るかは [`Ticket`] で一度だけ決め、両方で開いたり、どちらも開かなかったりしないようにする

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetLengthSid, GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_NONE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, SetNamedPipeHandleState,
    WaitNamedPipeW,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::core::{HSTRING, PWSTR};

/// まとめ先を探して、つながるまで待つ時間の合計。
const FIND_TIMEOUT: Duration = Duration::from_secs(3);
/// まとめ先が、受け取ったパスを画面に渡すのを待つ時間。
const DELIVER_TIMEOUT: Duration = Duration::from_secs(2);
/// 送る側が返事を待つ時間。まとめ先が断ると決めるまでの時間（[`DELIVER_TIMEOUT`]）より長くする。
const REPLY_TIMEOUT: Duration = Duration::from_secs(4);
/// 受け取るメッセージの大きさの上限。
const MAX_MESSAGE: usize = 1 << 20;
/// パイプのバッファの大きさ。
const BUFFER_SIZE: u32 = 64 * 1024;

const REPLY_OK: &[u8] = b"ok";
const REPLY_NO: &[u8] = b"no";

/// ハンドル（パイプ、プロセス、トークン）。手放すときに閉じる。
struct Handle(HANDLE);

// カーネルのオブジェクトのハンドルは、どのスレッドからでも使える。
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// パイプの名前。セッション、ユーザー、設定のファイルの場所ごとに違う名前になる。取れなければ `None`。
pub fn pipe_name(settings_file: &std::path::Path) -> Option<HSTRING> {
    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.ok()?;
    let sid = sid_string(&current_user_sid()?)?;
    let place = fnv1a(settings_file.to_string_lossy().to_lowercase().as_bytes());
    Some(HSTRING::from(format!(r"\\.\pipe\xml-viewer-{session}-{sid}-{place:016x}")))
}

/// FNV-1a（64 ビット）。設定の場所を、パイプの名前に入れられる短い値にする。
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// 起動したときの立場（[`start`]）。
pub enum Role {
    /// まとめ先になった。後からの起動を [`Listener::serve`] で受け付ける。
    Primary(Listener),
    /// まとめ先にパスを渡した。このまま終わる。
    HandedOver,
    /// 自分でウィンドウを出す。まとめ先に断られた、パイプを使えなかったなど。
    Alone,
}

/// 動いているまとめ先を探し、あればパスを渡す。無ければ自分がまとめ先になる。
pub fn start(name: &HSTRING, paths: &[PathBuf]) -> Role {
    let deadline = Instant::now() + FIND_TIMEOUT;
    loop {
        match Listener::claim(name) {
            Ok(Some(listener)) => return Role::Primary(listener),
            Ok(None) => {}
            Err(_) => return Role::Alone,
        }
        // ほかにまとめ先がある。つないでパスを渡す。
        match connect(name) {
            Connect::Connected(pipe) => return if send(pipe, paths) { Role::HandedOver } else { Role::Alone },
            Connect::Busy => {
                // 受け口がふさがっている（ほかの起動とやりとりしている）。空くまで待つ。
                let left = deadline.saturating_duration_since(Instant::now());
                let wait = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).clamp(1, 500);
                let _ = unsafe { WaitNamedPipeW(name, wait) };
            }
            // まとめ先が終わったところ。もう一度まとめ先になれるかを試す。
            Connect::Gone => std::thread::sleep(Duration::from_millis(20)),
            Connect::Failed => return Role::Alone,
        }
        if Instant::now() >= deadline {
            return Role::Alone;
        }
    }
}

/// まとめ先の受け口。
pub struct Listener {
    name: HSTRING,
    pipe: Handle,
}

impl Listener {
    /// まとめ先になる。ほかにまとめ先があれば `None`。
    pub fn claim(name: &HSTRING) -> windows::core::Result<Option<Self>> {
        match create_instance(name, true) {
            Ok(pipe) => Ok(Some(Self { name: name.clone(), pipe })),
            Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 後からの起動を受け付ける（別のスレッドで動く）。`accept` は渡されたパスを開くなら `true`、断るなら `false` を返す。
    pub fn serve(self, accept: impl Fn(Vec<PathBuf>) -> bool + Send + Sync + 'static) {
        let accept = Arc::new(accept);
        std::thread::spawn(move || {
            let mut current = self.pipe;
            loop {
                if let Err(e) = unsafe { ConnectNamedPipe(current.0, None) }
                    && e.code() != ERROR_PIPE_CONNECTED.to_hresult()
                {
                    // つながる前に相手が離れた。同じ受け口で待ち直す。
                    let _ = unsafe { DisconnectNamedPipe(current.0) };
                    continue;
                }
                // 次の受け口を先に作る。動いている間、パイプが無くならないように。
                let next = create_instance(&self.name, false);
                let accept = accept.clone();
                match next {
                    Ok(next) => {
                        let pipe = std::mem::replace(&mut current, next);
                        // 1 つの相手を待つ間に、ほかの起動を待たせないよう、相手ごとにスレッドを分ける。
                        std::thread::spawn(move || serve_one(pipe, accept.as_ref()));
                    }
                    Err(_) => {
                        // 次の受け口を作れなければ、今の相手に応えて受け付けをやめる。
                        serve_one(current, accept.as_ref());
                        return;
                    }
                }
            }
        });
    }
}

fn create_instance(name: &HSTRING, first: bool) -> windows::core::Result<Handle> {
    let mut open_mode = PIPE_ACCESS_DUPLEX;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            name,
            open_mode,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER_SIZE,
            BUFFER_SIZE,
            0,
            None,
        )
    };
    if handle.is_invalid() { Err(windows::core::Error::from_thread()) } else { Ok(Handle(handle)) }
}

/// 1 つの相手とやりとりする。パスを受け取り、開くか断るかを返す。
fn serve_one(pipe: Handle, accept: &(dyn Fn(Vec<PathBuf>) -> bool + Send + Sync)) {
    let ok = read_message(&pipe).and_then(|m| decode(&m)).is_some_and(accept);
    let _ = write_message(&pipe, if ok { REPLY_OK } else { REPLY_NO });
    unsafe {
        // 相手が返事を読み終えてから切る。
        let _ = FlushFileBuffers(pipe.0);
        let _ = DisconnectNamedPipe(pipe.0);
    }
}

enum Connect {
    Connected(Handle),
    /// 受け口がふさがっている。
    Busy,
    /// パイプが無い（まとめ先が終わった）。
    Gone,
    Failed,
}

fn connect(name: &HSTRING) -> Connect {
    // 相手には、こちらを名乗るだけで、なりすましはさせない（SECURITY_IDENTIFICATION）。
    let opened = unsafe {
        CreateFileW(
            name,
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )
    };
    match opened {
        Ok(handle) => Connect::Connected(Handle(handle)),
        Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() => Connect::Busy,
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Connect::Gone,
        Err(_) => Connect::Failed,
    }
}

/// まとめ先にパスを送る。まとめ先が開くと答えたら `true`。
fn send(pipe: Handle, paths: &[PathBuf]) -> bool {
    let mut server = 0u32;
    if unsafe { GetNamedPipeServerProcessId(pipe.0, &mut server) }.is_err() || !same_user(server) {
        return false;
    }
    if unsafe { SetNamedPipeHandleState(pipe.0, Some(&PIPE_READMODE_MESSAGE), None, None) }.is_err() {
        return false;
    }
    // まとめ先がウィンドウを前に出せるようにする。前に出す権利は、利用者が今起動したこちらが持っている。
    let _ = unsafe { AllowSetForegroundWindow(server) };
    let message = encode(paths);
    // まとめ先が応えないときに待ち続けないよう、やりとりは別のスレッドで行う。
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ok = write_message(&pipe, &message).is_ok() && read_message(&pipe).is_some_and(|r| r == REPLY_OK);
        let _ = tx.send(ok);
    });
    rx.recv_timeout(REPLY_TIMEOUT).unwrap_or(false)
}

fn encode(paths: &[PathBuf]) -> Vec<u8> {
    let list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    serde_json::to_vec(&list).unwrap_or_default()
}

fn decode(message: &[u8]) -> Option<Vec<PathBuf>> {
    let list: Vec<String> = serde_json::from_slice(message).ok()?;
    Some(list.into_iter().map(PathBuf::from).collect())
}

fn write_message(pipe: &Handle, bytes: &[u8]) -> windows::core::Result<()> {
    let mut written = 0u32;
    unsafe { WriteFile(pipe.0, Some(bytes), Some(&mut written), None) }
}

/// メッセージを 1 つ読む。読めない、または大きすぎれば `None`。
fn read_message(pipe: &Handle) -> Option<Vec<u8>> {
    let mut message = Vec::new();
    let mut buf = vec![0u8; BUFFER_SIZE as usize];
    loop {
        let mut read = 0u32;
        let result = unsafe { ReadFile(pipe.0, Some(&mut buf), Some(&mut read), None) };
        message.extend_from_slice(&buf[..read as usize]);
        if message.len() > MAX_MESSAGE {
            return None;
        }
        match result {
            Ok(()) => return Some(message),
            Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => continue,
            Err(_) => return None,
        }
    }
}

/// `pid` のプロセスが、このプロセスと同じユーザーで動いているか。
fn same_user(pid: u32) -> bool {
    let Some(mine) = current_user_sid() else { return false };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let process = Handle(process);
    process_user_sid(process.0).is_some_and(|sid| sid == mine)
}

fn current_user_sid() -> Option<Vec<u8>> {
    process_user_sid(unsafe { GetCurrentProcess() })
}

/// プロセスを動かしているユーザーの SID（バイト列）。
fn process_user_sid(process: HANDLE) -> Option<Vec<u8>> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let token = Handle(token);
    let mut len = 0u32;
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut len) };
    if len == 0 {
        return None;
    }
    // TOKEN_USER はポインタを含むので、8 バイト境界にそろえた領域に受け取る。
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    unsafe { GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len) }.ok()?;
    let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
    let sid = user.User.Sid;
    let sid_len = unsafe { GetLengthSid(sid) } as usize;
    Some(unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), sid_len) }.to_vec())
}

/// SID の文字列（`S-1-5-21-…`）。
fn sid_string(sid: &[u8]) -> Option<String> {
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(PSID(sid.as_ptr().cast_mut().cast()), &mut text) }.ok()?;
    let result = unsafe { text.to_string() }.ok();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
    }
    result
}

/// まとめ先が開くか断るかを、一度だけ決める札。
///
/// まとめ先の受け付けのスレッドは、ウィンドウが開くのを [`DELIVER_TIMEOUT`] まで待ち、
/// 間に合わなければ断ったことにする。ウィンドウは開く前に [`Ticket::take`] で確かめ、
/// もう断った後なら開かない（送った側が自分のウィンドウで開いている）。
#[derive(Clone)]
pub struct Ticket(Arc<(Mutex<TicketState>, Condvar)>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum TicketState {
    Waiting,
    Taken,
    Dropped,
}

impl Ticket {
    pub fn new() -> Self {
        Self(Arc::new((Mutex::new(TicketState::Waiting), Condvar::new())))
    }

    /// ウィンドウの側: 開いてよければ `true`。受け付けの側がもう断っていれば `false`。
    pub fn take(&self) -> bool {
        let (lock, cond) = &*self.0;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if *state != TicketState::Waiting {
            return false;
        }
        *state = TicketState::Taken;
        cond.notify_all();
        true
    }

    /// 受け付けの側: ウィンドウが時間内に引き受けたら `true`。間に合わなければ断ったことにして `false`。
    pub fn wait(&self) -> bool {
        self.wait_for(DELIVER_TIMEOUT)
    }

    fn wait_for(&self, timeout: Duration) -> bool {
        let (lock, cond) = &*self.0;
        let state = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (mut state, _) =
            cond.wait_timeout_while(state, timeout, |s| *s == TicketState::Waiting).unwrap_or_else(|e| e.into_inner());
        if *state == TicketState::Waiting {
            *state = TicketState::Dropped;
        }
        *state == TicketState::Taken
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// テストごとに違うパイプの名前。
    fn test_name() -> HSTRING {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        HSTRING::from(format!(r"\\.\pipe\xml-viewer-test-{}-{n}", std::process::id()))
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!(r"C:\docs\{n}"))).collect()
    }

    /// 受け取ったパスを記録するまとめ先。
    fn record(listener: Listener, answer: bool) -> Arc<Mutex<Vec<PathBuf>>> {
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        listener.serve(move |p| {
            sink.lock().unwrap().extend(p);
            answer
        });
        received
    }

    #[test]
    fn first_launch_becomes_primary_and_later_ones_hand_over() {
        let name = test_name();
        let Role::Primary(listener) = start(&name, &[]) else { panic!("まとめ先になれない") };
        let received = record(listener, true);
        assert!(matches!(start(&name, &paths(&["a.zip", "日本語 b.xml"])), Role::HandedOver));
        assert!(matches!(start(&name, &[]), Role::HandedOver));
        assert!(matches!(start(&name, &paths(&["c.zip"])), Role::HandedOver));
        assert_eq!(*received.lock().unwrap(), paths(&["a.zip", "日本語 b.xml", "c.zip"]));
    }

    #[test]
    fn refused_hand_over_opens_own_window() {
        let name = test_name();
        let Role::Primary(listener) = start(&name, &[]) else { panic!("まとめ先になれない") };
        record(listener, false);
        assert!(matches!(start(&name, &paths(&["a.zip"])), Role::Alone));
    }

    #[test]
    fn next_launch_becomes_primary_after_primary_ends() {
        let name = test_name();
        let Role::Primary(listener) = start(&name, &[]) else { panic!("まとめ先になれない") };
        drop(listener);
        assert!(matches!(start(&name, &[]), Role::Primary(_)));
    }

    #[test]
    fn simultaneous_launches_elect_one_primary() {
        const LAUNCHES: usize = 8;
        let name = test_name();
        let barrier = Arc::new(Barrier::new(LAUNCHES));
        let received = Arc::new(Mutex::new(Vec::new()));
        let threads: Vec<_> = (0..LAUNCHES)
            .map(|i| {
                let (name, barrier, received) = (name.clone(), barrier.clone(), received.clone());
                std::thread::spawn(move || {
                    let mine = paths(&[&format!("{i}.zip")]);
                    barrier.wait();
                    match start(&name, &mine) {
                        Role::Primary(listener) => {
                            received.lock().unwrap().extend(mine);
                            let sink = received.clone();
                            listener.serve(move |p| {
                                sink.lock().unwrap().extend(p);
                                true
                            });
                            "primary"
                        }
                        Role::HandedOver => "handed",
                        Role::Alone => "alone",
                    }
                })
            })
            .collect();
        let roles: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(roles.iter().filter(|r| **r == "primary").count(), 1, "{roles:?}");
        assert_eq!(roles.iter().filter(|r| **r == "handed").count(), LAUNCHES - 1, "{roles:?}");
        let mut got = received.lock().unwrap().clone();
        got.sort();
        let mut want: Vec<_> = (0..LAUNCHES).flat_map(|i| paths(&[&format!("{i}.zip")])).collect();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn ticket_decides_once() {
        let ticket = Ticket::new();
        assert!(ticket.take());
        assert!(!ticket.take());
        assert!(ticket.wait_for(Duration::from_millis(10)));

        let late = Ticket::new();
        assert!(!late.wait_for(Duration::from_millis(10)));
        assert!(!late.take(), "断った後に開いてはいけない");
    }

    #[test]
    fn stalled_primary_lets_launch_open_its_own_window() {
        let name = test_name();
        let Role::Primary(listener) = start(&name, &[]) else { panic!("まとめ先になれない") };
        // ウィンドウが固まっていて、札を引き受けない。
        let tickets = Arc::new(Mutex::new(Vec::new()));
        let kept = tickets.clone();
        listener.serve(move |_| {
            let ticket = Ticket::new();
            kept.lock().unwrap().push(ticket.clone());
            ticket.wait()
        });
        assert!(matches!(start(&name, &paths(&["a.zip"])), Role::Alone));
        // 後から動き出しても、もう開かない。
        assert!(!tickets.lock().unwrap()[0].take());
    }

    #[test]
    fn name_depends_on_settings_place() {
        let a = pipe_name(std::path::Path::new(r"C:\Users\u\AppData\Roaming\xml-viewer\settings.json")).unwrap();
        let b = pipe_name(std::path::Path::new(r"C:\Users\u\AppData\Roaming\XML-Viewer\Settings.json")).unwrap();
        let c = pipe_name(std::path::Path::new(r"C:\Temp\verify\xml-viewer\settings.json")).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.to_string().starts_with(r"\\.\pipe\xml-viewer-"));
    }

    #[test]
    fn checks_user_of_process() {
        assert!(same_user(std::process::id()));
        assert!(!same_user(0));
    }
}
