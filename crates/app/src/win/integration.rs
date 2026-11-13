//! エクスプローラー連携（「送る」・右クリックメニュー・「プログラムから開く」）の登録と解除。
//!
//! どれも今のユーザーだけに効く登録（`HKEY_CURRENT_USER` とユーザーの「送る」フォルダ）で、
//! 管理者権限は要らない。どの入口も、選ばれたファイルのパスを起動引数で渡すだけ。

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ};
use windows::Win32::UI::Shell::{
    FOLDERID_SendTo, IShellLinkW, KF_FLAG_DEFAULT, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify,
    SHGetKnownFolderPath, ShellLink,
};
use windows::core::{HSTRING, Interface};
use windows_registry::CURRENT_USER;

use super::com::ensure_sta;
use crate::api::{IntegrationItem, IntegrationState};

/// 項目のキー（画面とやりとりする名前）。
pub const SEND_TO: &str = "sendTo";
pub const CONTEXT_MENU: &str = "contextMenu";
pub const OPEN_WITH: &str = "openWith";

/// 右クリックメニューを足す拡張子。
const EXTENSIONS: [&str; 2] = [".zip", ".xml"];
/// 右クリックメニューの項目のキー名。
const VERB: &str = "XmlViewer.Open";
const MENU_LABEL: &str = "XML Viewer で開く";
const APP_NAME: &str = "XML Viewer";
const SHORTCUT_NAME: &str = "XML Viewer.lnk";

/// エクスプローラー連携。
pub struct Integration {
    exe: PathBuf,
    /// `HKEY_CURRENT_USER` の下の、ファイルの種類の登録先（通常は `Software\Classes`）。
    classes: String,
    /// 「送る」フォルダ。取れなければ `None`。
    send_to: Option<PathBuf>,
}

impl Integration {
    /// 今動いている exe で登録する。
    pub fn current() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        Self { exe, classes: r"Software\Classes".to_string(), send_to: send_to_folder() }
    }

    /// 登録先を差し替える（テスト用）。
    #[cfg(test)]
    pub fn with_roots(exe: PathBuf, classes: String, send_to: PathBuf) -> Self {
        Self { exe, classes, send_to: Some(send_to) }
    }

    fn exe_name(&self) -> String {
        self.exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "xml-viewer.exe".into())
    }

    fn command(&self) -> String {
        format!("\"{}\" \"%1\"", self.exe.display())
    }

    fn menu_key(&self, ext: &str) -> String {
        format!(r"{}\SystemFileAssociations\{ext}\shell\{VERB}", self.classes)
    }

    fn app_key(&self) -> String {
        format!(r"{}\Applications\{}", self.classes, self.exe_name())
    }

    fn open_with_list_key(&self, ext: &str) -> String {
        format!(r"{}\{ext}\OpenWithList\{}", self.classes, self.exe_name())
    }

    fn shortcut_path(&self) -> Option<PathBuf> {
        self.send_to.as_ref().map(|d| d.join(SHORTCUT_NAME))
    }

    /// 3 つの項目の状態。
    pub fn status(&self) -> Vec<IntegrationItem> {
        vec![
            IntegrationItem { key: SEND_TO.into(), state: self.send_to_state() },
            IntegrationItem { key: CONTEXT_MENU.into(), state: self.context_menu_state() },
            IntegrationItem { key: OPEN_WITH.into(), state: self.open_with_state() },
        ]
    }

    /// 項目を入れる・外す。入れるときは、今の exe の場所で登録し直す。
    pub fn set(&self, key: &str, enabled: bool) -> Result<(), String> {
        let result = match (key, enabled) {
            (SEND_TO, true) => self.add_send_to(),
            (SEND_TO, false) => self.remove_send_to(),
            (CONTEXT_MENU, true) => self.add_context_menu(),
            (CONTEXT_MENU, false) => self.remove_context_menu(),
            (OPEN_WITH, true) => self.add_open_with(),
            (OPEN_WITH, false) => self.remove_open_with(),
            _ => return Err(format!("知らない項目です: {key}")),
        };
        if key != SEND_TO {
            // エクスプローラーに、関連付けが変わったことを知らせる。
            // SAFETY: 引数を使わない通知。
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        }
        result
    }

    fn same_command(&self, value: Option<String>) -> Option<bool> {
        value.map(|v| v.eq_ignore_ascii_case(&self.command()))
    }

    fn state_of(results: &[Option<bool>]) -> IntegrationState {
        if results.iter().all(|r| *r == Some(true)) {
            IntegrationState::On
        } else if results.iter().any(Option::is_some) {
            IntegrationState::Stale
        } else {
            IntegrationState::Off
        }
    }

    // ---- 右クリックメニュー ----

    fn context_menu_state(&self) -> IntegrationState {
        let results: Vec<Option<bool>> = EXTENSIONS
            .iter()
            .map(|ext| self.same_command(read_default(&format!(r"{}\command", self.menu_key(ext)))))
            .collect();
        Self::state_of(&results)
    }

    fn add_context_menu(&self) -> Result<(), String> {
        for ext in EXTENSIONS {
            let key = CURRENT_USER.create(self.menu_key(ext)).map_err(reg_error)?;
            key.set_string("", MENU_LABEL).map_err(reg_error)?;
            let command = key.create("command").map_err(reg_error)?;
            command.set_string("", self.command()).map_err(reg_error)?;
        }
        Ok(())
    }

    fn remove_context_menu(&self) -> Result<(), String> {
        for ext in EXTENSIONS {
            remove_tree(&self.menu_key(ext))?;
        }
        Ok(())
    }

    // ---- 「プログラムから開く」 ----

    fn open_with_state(&self) -> IntegrationState {
        let command = self.same_command(read_default(&format!(r"{}\shell\open\command", self.app_key())));
        Self::state_of(&[command])
    }

    fn add_open_with(&self) -> Result<(), String> {
        let app = CURRENT_USER.create(self.app_key()).map_err(reg_error)?;
        app.set_string("FriendlyAppName", APP_NAME).map_err(reg_error)?;
        let command = app.create(r"shell\open\command").map_err(reg_error)?;
        command.set_string("", self.command()).map_err(reg_error)?;
        let types = app.create("SupportedTypes").map_err(reg_error)?;
        for ext in EXTENSIONS {
            types.set_string(ext, "").map_err(reg_error)?;
            CURRENT_USER.create(self.open_with_list_key(ext)).map_err(reg_error)?;
        }
        Ok(())
    }

    fn remove_open_with(&self) -> Result<(), String> {
        remove_tree(&self.app_key())?;
        for ext in EXTENSIONS {
            remove_tree(&self.open_with_list_key(ext))?;
        }
        Ok(())
    }

    // ---- 「送る」 ----

    fn send_to_state(&self) -> IntegrationState {
        let Some(path) = self.shortcut_path().filter(|p| p.exists()) else {
            return IntegrationState::Off;
        };
        match read_shortcut_target(&path) {
            Some(target) if xml_viewer_core::same_path(&target, &self.exe) => IntegrationState::On,
            _ => IntegrationState::Stale,
        }
    }

    fn add_send_to(&self) -> Result<(), String> {
        let path = self.shortcut_path().ok_or("「送る」フォルダが見つかりません")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        ensure_sta()?;
        let err = |e: windows::core::Error| format!("ショートカットを作れませんでした（{}）", e.message());
        // SAFETY: COM はこのスレッドで初期化済み。文字列は呼び出しの間生きている。
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(err)?;
            link.SetPath(&HSTRING::from(self.exe.as_os_str())).map_err(err)?;
            link.SetDescription(&HSTRING::from(MENU_LABEL)).map_err(err)?;
            let file: IPersistFile = link.cast().map_err(err)?;
            file.Save(&HSTRING::from(path.as_os_str()), true).map_err(err)?;
        }
        Ok(())
    }

    fn remove_send_to(&self) -> Result<(), String> {
        match self.shortcut_path() {
            Some(path) if path.exists() => std::fs::remove_file(path).map_err(|e| e.to_string()),
            _ => Ok(()),
        }
    }
}

/// ユーザーの「送る」フォルダ。
fn send_to_folder() -> Option<PathBuf> {
    // SAFETY: 戻り値の文字列は CoTaskMemFree で解放する。
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_SendTo, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        text.map(PathBuf::from)
    }
}

/// ショートカットの指す先。
fn read_shortcut_target(path: &Path) -> Option<PathBuf> {
    ensure_sta().ok()?;
    // SAFETY: COM はこのスレッドで初期化済み。バッファは呼び出しの間生きている。
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(path.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 32768];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

fn read_default(path: &str) -> Option<String> {
    CURRENT_USER.open(path).ok()?.get_string("").ok()
}

fn remove_tree(path: &str) -> Result<(), String> {
    match CURRENT_USER.remove_tree(path) {
        Ok(()) => Ok(()),
        // 無ければ何もしない。windows-registry と windows で windows-result の版が違っても比べられるよう、値で比べる。
        Err(e) if e.code().0 == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.to_hresult().0 => Ok(()),
        Err(e) => Err(reg_error(e)),
    }
}

/// レジストリのエラーを画面に出す文にする。
///
/// windows-registry が使う windows-result の版に縛られないよう、表示できるエラーなら何でも受ける。
fn reg_error(e: impl std::fmt::Display) -> String {
    format!("レジストリに書き込めませんでした（{e}）")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本物の登録先に触れないよう、テスト専用のキーとフォルダを使う。
    fn integration(name: &str, exe: &str) -> (Integration, String, PathBuf) {
        let root = format!(r"Software\xml-viewer-test-{name}-{}", std::process::id());
        let send_to = std::env::temp_dir().join(format!("xmlv-sendto-{name}-{}", std::process::id()));
        let _ = CURRENT_USER.remove_tree(&root);
        let _ = std::fs::remove_dir_all(&send_to);
        let i = Integration::with_roots(PathBuf::from(exe), format!(r"{root}\Classes"), send_to.clone());
        (i, root, send_to)
    }

    fn states(i: &Integration) -> Vec<IntegrationState> {
        i.status().into_iter().map(|s| s.state).collect()
    }

    #[test]
    fn registers_and_unregisters_each_item() {
        let exe = std::env::current_exe().unwrap();
        let (i, root, send_to) = integration("roundtrip", &exe.to_string_lossy());
        assert_eq!(states(&i), [IntegrationState::Off; 3]);

        for key in [SEND_TO, CONTEXT_MENU, OPEN_WITH] {
            i.set(key, true).unwrap();
        }
        assert_eq!(states(&i), [IntegrationState::On; 3]);
        let command = read_default(&format!(r"{}\command", i.menu_key(".zip"))).unwrap();
        assert_eq!(command, format!("\"{}\" \"%1\"", exe.display()));
        assert!(CURRENT_USER.open(i.open_with_list_key(".xml")).is_ok());

        for key in [SEND_TO, CONTEXT_MENU, OPEN_WITH] {
            i.set(key, false).unwrap();
            // 2 回外してもエラーにならない。
            i.set(key, false).unwrap();
        }
        assert_eq!(states(&i), [IntegrationState::Off; 3]);
        assert!(!send_to.join(SHORTCUT_NAME).exists());

        let _ = CURRENT_USER.remove_tree(&root);
        let _ = std::fs::remove_dir_all(&send_to);
    }

    #[test]
    fn detects_registration_of_moved_exe() {
        let exe = std::env::current_exe().unwrap();
        let (old, root, send_to) = integration("moved", r"C:\old\place\xml-viewer.exe");
        for key in [CONTEXT_MENU, OPEN_WITH] {
            old.set(key, true).unwrap();
        }
        // 「送る」はショートカットの先が実在しないと作れないので、実在する exe で作ってから比べる。
        let other = Integration::with_roots(exe.clone(), old.classes.clone(), send_to.clone());
        other.set(SEND_TO, true).unwrap();

        let now = Integration::with_roots(
            PathBuf::from(r"C:\new\place\xml-viewer.exe"),
            old.classes.clone(),
            send_to.clone(),
        );
        assert_eq!(states(&now), [IntegrationState::Stale; 3]);

        // 登録し直すと、今の場所で有効になる。
        let current = Integration::with_roots(exe, old.classes.clone(), send_to.clone());
        for key in [SEND_TO, CONTEXT_MENU, OPEN_WITH] {
            current.set(key, true).unwrap();
        }
        assert_eq!(states(&current), [IntegrationState::On; 3]);

        let _ = CURRENT_USER.remove_tree(&root);
        let _ = std::fs::remove_dir_all(&send_to);
    }

    #[test]
    fn rejects_unknown_key() {
        let (i, _, _) = integration("unknown", r"C:\x.exe");
        assert!(i.set("desktop", true).is_err());
    }
}
