//! ファイルや URL を、既定のアプリで開く。

use std::path::Path;

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use super::com::ensure_sta;

/// ShellExecute のエラー: 関連付けが無い / 不完全。
const SE_ERR_NOASSOC: isize = 31;
const SE_ERR_ASSOCINCOMPLETE: isize = 27;

/// ファイルを既定のアプリで開く。関連付けが無ければ「プログラムから開く」を出す。
pub fn open_file(path: &Path) -> Result<(), String> {
    ensure_sta()?;
    let file = HSTRING::from(path.as_os_str());
    match execute(w!("open"), &file) {
        Ok(()) => Ok(()),
        Err(code) if code == SE_ERR_NOASSOC || code == SE_ERR_ASSOCINCOMPLETE => {
            execute(w!("openas"), &file).map_err(describe)
        }
        Err(code) => Err(describe(code)),
    }
}

/// URL を既定のブラウザで開く。`http` と `https` だけを受け付ける。
pub fn open_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("http と https 以外のリンクは開きません".to_string());
    }
    ensure_sta()?;
    execute(w!("open"), &HSTRING::from(url)).map_err(describe)
}

fn execute(verb: PCWSTR, target: &HSTRING) -> Result<(), isize> {
    // SAFETY: 文字列はどれも呼び出しの間生きている。
    let result = unsafe { ShellExecuteW(None, verb, target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    let code = result.0 as isize;
    if code > 32 { Ok(()) } else { Err(code) }
}

fn describe(code: isize) -> String {
    match code {
        2 | 3 => "ファイルが見つかりません".to_string(),
        5 => "アクセスが拒否されました".to_string(),
        SE_ERR_NOASSOC | SE_ERR_ASSOCINCOMPLETE => "このファイルを開くアプリが登録されていません".to_string(),
        _ => format!("開けませんでした（エラー {code}）"),
    }
}
