//! zip を展開する一時フォルダの管理。
//!
//! 起動ごとに `{root}/{起動ごとの ID}/` を作り、zip ごとに `{連番}/` を切って展開する。
//! 公文書は個人情報を含むので、閉じたとき・終了するときに消し、
//! 異常終了などで残ったものは次の起動時に [`sweep_stale`] で消す。
//!
//! 複数のウィンドウ（プロセス）が同時に動くので、使用中のフォルダを消さないようにする。
//! 起動ごとのフォルダに置いたロック用のファイルを、動いている間ずっとロックしておき、
//! 掃除ではロックを取れたフォルダ（＝持ち主がもう動いていない）だけを消す。

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// ロック用のファイルの名前。
const LOCK_FILE: &str = ".lock";

/// ロック用のファイルが無いフォルダを、作成途中とみなす猶予。
const CREATING_GRACE: Duration = Duration::from_secs(60);

/// 起動ごとの一時フォルダ。破棄すると中身ごと消える。
#[derive(Debug)]
pub struct TempSession {
    dir: PathBuf,
    lock: Option<File>,
    next: AtomicU32,
}

impl TempSession {
    /// `root` の下に、この起動用のフォルダを作ってロックする。
    pub fn create(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let base = format!("{}-{millis}", std::process::id());
        for attempt in 0..100u32 {
            let dir = root.join(if attempt == 0 { base.clone() } else { format!("{base}-{attempt}") });
            match fs::create_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
            let lock = OpenOptions::new().read(true).write(true).create_new(true).open(dir.join(LOCK_FILE))?;
            lock.try_lock().map_err(|e| match e {
                TryLockError::Error(e) => e,
                TryLockError::WouldBlock => io::Error::other("ロック用のファイルをロックできませんでした"),
            })?;
            return Ok(Self { dir, lock: Some(lock), next: AtomicU32::new(1) });
        }
        Err(io::Error::other("一時フォルダを作れませんでした"))
    }

    /// この起動用のフォルダ。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// zip 1 つぶんの展開先を新しく作る。
    pub fn new_extract_dir(&self) -> io::Result<PathBuf> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let dir = self.dir.join(n.to_string());
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

impl Drop for TempSession {
    fn drop(&mut self) {
        // 先にロック用のファイルを閉じてから消す（開いたままだと Windows では消せない）。
        drop(self.lock.take());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// 掃除の結果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// 消したフォルダの数。
    pub removed: usize,
    /// 使用中のため残したフォルダの数。
    pub in_use: usize,
    /// 消そうとして消しきれなかったフォルダの数（ほかのアプリがファイルを開いている場合など）。
    pub failed: usize,
}

/// `root` の下に残っている、持ち主のいない一時フォルダを消す。
pub fn sweep_stale(root: &Path) -> SweepReport {
    let mut report = SweepReport::default();
    let Ok(entries) = fs::read_dir(root) else {
        return report;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        match owner_state(&path) {
            Owner::Alive => report.in_use += 1,
            Owner::Gone(lock) => {
                drop(lock);
                match fs::remove_dir_all(&path) {
                    Ok(()) => report.removed += 1,
                    Err(_) => report.failed += 1,
                }
            }
        }
    }
    report
}

enum Owner {
    Alive,
    /// 持ち主がいない。ロック用のファイルがあれば、消すまでロックを保持しておく。
    Gone(Option<File>),
}

fn owner_state(dir: &Path) -> Owner {
    let lock_path = dir.join(LOCK_FILE);
    match OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(file) => match file.try_lock() {
            Ok(()) => Owner::Gone(Some(file)),
            Err(_) => Owner::Alive,
        },
        // Windows では、ほかのプロセスが共有なしで開いていると開けない。使用中とみなす。
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Owner::Alive,
        Err(_) => {
            // ロック用のファイルが無い。作成途中かもしれないので、新しいものは残す。
            let age = fs::metadata(dir)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .unwrap_or_default();
            if age < CREATING_GRACE { Owner::Alive } else { Owner::Gone(None) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("xmlv-temp-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn session_creates_dirs_and_removes_on_drop() {
        let root = test_root("drop");
        let session = TempSession::create(&root).unwrap();
        let a = session.new_extract_dir().unwrap();
        let b = session.new_extract_dir().unwrap();
        assert_ne!(a, b);
        fs::write(a.join("x.xml"), "<x/>").unwrap();
        let dir = session.dir().to_path_buf();
        drop(session);
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sweep_keeps_live_sessions_and_removes_orphans() {
        let root = test_root("sweep");
        let live = TempSession::create(&root).unwrap();
        fs::write(live.new_extract_dir().unwrap().join("a.xml"), "<a/>").unwrap();

        // 異常終了したプロセスの残り（ロック用のファイルはあるが、誰もロックしていない）。
        let orphan = root.join("1-1");
        fs::create_dir_all(orphan.join("1")).unwrap();
        fs::write(orphan.join(LOCK_FILE), "").unwrap();
        fs::write(orphan.join("1").join("b.xml"), "<b/>").unwrap();

        // 作成途中に見えるもの（ロック用のファイルがまだ無く、新しい）。
        let creating = root.join("2-2");
        fs::create_dir_all(&creating).unwrap();

        let report = sweep_stale(&root);
        assert_eq!(report, SweepReport { removed: 1, in_use: 2, failed: 0 });
        assert!(live.dir().join("1").join("a.xml").exists());
        assert!(!orphan.exists());
        assert!(creating.exists());

        drop(live);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sweep_on_missing_root_does_nothing() {
        assert_eq!(sweep_stale(Path::new("/nonexistent/xmlv")), SweepReport::default());
    }
}
