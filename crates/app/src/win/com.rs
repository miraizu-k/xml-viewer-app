//! COM の初期化。

use std::cell::Cell;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

thread_local! {
    static READY: Cell<bool> = const { Cell::new(false) };
}

/// このスレッドで COM を使えるようにする（シングルスレッド アパートメント）。
/// スレッドが終わるまで解除しない。何度呼んでもよい。
pub fn ensure_sta() -> Result<(), String> {
    if READY.get() {
        return Ok(());
    }
    // SAFETY: COM の初期化。同じスレッドで何度呼んでもよい。
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        return Err(format!("COM を初期化できませんでした（{hr:?}）"));
    }
    READY.set(true);
    Ok(())
}
