//! ファイル選択ダイアログとメッセージボックス。

use std::path::PathBuf;

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FileOpenDialog, IFileOpenDialog,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HRESULT, HSTRING, PCWSTR};

use super::com::ensure_sta;

/// 「開く」ダイアログを出し、選ばれたファイルのパスを返す。取り消されたら空。
///
/// `owner` はダイアログの持ち主になるウィンドウ。呼び出し元のスレッドで、ダイアログが閉じるまで待つ。
pub fn pick_files(owner: isize) -> Result<Vec<PathBuf>, String> {
    ensure_sta()?;
    let err = |e: windows::core::Error| format!("ダイアログを出せませんでした（{}）", e.message());
    let filter_name = HSTRING::from("zip / XML");
    let filter_spec = HSTRING::from("*.zip;*.xml");
    let all_name = HSTRING::from("すべてのファイル");
    let all_spec = HSTRING::from("*.*");
    let title = HSTRING::from("開く");
    let filters = [
        COMDLG_FILTERSPEC { pszName: PCWSTR(filter_name.as_ptr()), pszSpec: PCWSTR(filter_spec.as_ptr()) },
        COMDLG_FILTERSPEC { pszName: PCWSTR(all_name.as_ptr()), pszSpec: PCWSTR(all_spec.as_ptr()) },
    ];
    // SAFETY: COM はこのスレッドで初期化済み。文字列は呼び出しの間生きている。
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        let options = dialog.GetOptions().map_err(err)?;
        dialog
            .SetOptions(options | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_FORCEFILESYSTEM)
            .map_err(err)?;
        dialog.SetFileTypes(&filters).map_err(err)?;
        dialog.SetTitle(&title).map_err(err)?;
        if let Err(e) = dialog.Show(Some(HWND(owner as *mut _))) {
            if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(Vec::new());
            }
            return Err(err(e));
        }
        let items = dialog.GetResults().map_err(err)?;
        let count = items.GetCount().map_err(err)?;
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let item = items.GetItemAt(i).map_err(err)?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(err)?;
            paths.push(PathBuf::from(name.to_string().unwrap_or_default()));
            CoTaskMemFree(Some(name.0 as *const _));
        }
        Ok(paths)
    }
}

/// エラーのメッセージボックスを出す（起動に失敗したときに使う）。
pub fn error_box(text: &str) {
    // SAFETY: 文字列は呼び出しの間生きている。
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("XML Viewer"), MB_OK | MB_ICONERROR);
    }
}
