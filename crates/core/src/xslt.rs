//! XML と XSL から HTML を作る。
//!
//! Windows では、OS に入っている MSXML 6 を COM 経由で呼ぶ（[`Msxml`]）。
//! プロパティは既定のまま使う（スクリプト不可、`document()` 不可、外部参照なし、DTD 不可）。
//! そのため `xsl:import` / `xsl:include` を使う XSL と、DOCTYPE 宣言を持つ XML は変換できない。
//!
//! 変換の中身は [`Transformer`] の裏に閉じ込めてあり、別の実装に差し替えられる。

use std::fmt;
use std::path::Path;

use serde::Serialize;

/// 変換に失敗した理由。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransformError {
    /// 問題のあったファイルの名前（XML か XSL）。
    pub file: Option<String>,
    /// 行（1 始まり）。
    pub line: Option<u32>,
    /// 桁（1 始まり）。
    pub column: Option<u32>,
    /// 理由（MSXML が返す説明）。
    pub reason: String,
}

impl TransformError {
    /// ファイル名と理由だけのエラーを作る。
    pub fn new(file: Option<&Path>, reason: impl Into<String>) -> Self {
        Self {
            file: file.and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()),
            line: None,
            column: None,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}")?;
            if let Some(line) = self.line {
                write!(f, "（{line} 行目")?;
                if let Some(col) = self.column {
                    write!(f, "、{col} 文字目")?;
                }
                write!(f, "）")?;
            }
            write!(f, ": ")?;
        }
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for TransformError {}

/// XML と XSL から HTML 文字列を作るもの。
pub trait Transformer: Sync {
    fn transform(&self, xml: &Path, xsl: &Path) -> Result<String, TransformError>;
}

/// MSXML 6 による変換。
#[derive(Debug, Default, Clone, Copy)]
pub struct Msxml;

#[cfg(not(windows))]
impl Transformer for Msxml {
    fn transform(&self, _xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        Err(TransformError::new(Some(xsl), "この OS では変換できません（MSXML は Windows 専用です）"))
    }
}

#[cfg(windows)]
impl Transformer for Msxml {
    fn transform(&self, xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        msxml::transform(xml, xsl)
    }
}

#[cfg(windows)]
mod msxml {
    use std::cell::Cell;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::Data::Xml::MsXml::{DOMDocument60, IXMLDOMDocument2};
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, VARIANT_FALSE};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::core::BSTR;

    use super::TransformError;

    thread_local! {
        static COM_READY: Cell<bool> = const { Cell::new(false) };
    }

    /// このスレッドで COM を使えるようにする。スレッドが終わるまで解除しない。
    fn ensure_com() -> Result<(), TransformError> {
        if COM_READY.get() {
            return Ok(());
        }
        // SAFETY: COM の初期化。同じスレッドで何度呼んでもよい。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // 別の方式ですでに初期化済み（RPC_E_CHANGED_MODE）でも、MSXML はそのまま使える。
        if hr.is_err() && hr != RPC_E_CHANGED_MODE {
            return Err(TransformError::new(None, format!("COM を初期化できませんでした（{hr:?}）")));
        }
        COM_READY.set(true);
        Ok(())
    }

    pub(super) fn transform(xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        ensure_com()?;
        let source = load(xml)?;
        let style = load(xsl)?;
        // SAFETY: どちらも同じスレッドで作った DOM。
        let html = unsafe { source.transformNode(&style) }
            .map_err(|e| TransformError::new(Some(xsl), clean_message(&e.message())))?;
        Ok(html.to_string())
    }

    /// ファイルを同期で読み込んだ DOM を作る。
    fn load(path: &Path) -> Result<IXMLDOMDocument2, TransformError> {
        let fail = |e: windows::core::Error| TransformError::new(Some(path), clean_message(&e.message()));
        // SAFETY: COM はこのスレッドで初期化済み。
        unsafe {
            let doc: IXMLDOMDocument2 = CoCreateInstance(&DOMDocument60, None, CLSCTX_INPROC_SERVER).map_err(fail)?;
            doc.Setasync(VARIANT_FALSE).map_err(fail)?;
            let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            let location = VARIANT::from(BSTR::from_wide(&wide));
            let ok = doc.load(&location).map_err(fail)?;
            if ok == VARIANT_FALSE {
                let err = doc.parseError().map_err(fail)?;
                let reason = err.reason().map(|r| r.to_string()).unwrap_or_default();
                let line = err.line().ok().filter(|&n| n > 0).map(|n| n as u32);
                let column = err.linepos().ok().filter(|&n| n > 0).map(|n| n as u32);
                let mut e = TransformError::new(Some(path), clean_message(&reason));
                e.line = line;
                e.column = column;
                return Err(e);
            }
            Ok(doc)
        }
    }

    /// MSXML の説明文の前後の空白と改行を整える。
    fn clean_message(message: &str) -> String {
        let text = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() { "原因不明のエラーです".to_string() } else { text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_error_with_position() {
        let mut e = TransformError::new(Some(Path::new("a/7140001.xsl")), "終了タグが一致しません。");
        e.line = Some(38);
        e.column = Some(12);
        assert_eq!(e.to_string(), "7140001.xsl（38 行目、12 文字目）: 終了タグが一致しません。");
        assert_eq!(TransformError::new(None, "x").to_string(), "x");
    }
}
