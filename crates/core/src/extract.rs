//! zip の安全な展開。

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use zip::ZipArchive;
use zip::result::ZipError;

use crate::source::{Limits, OpenError};

/// 展開の結果。
#[derive(Debug, Default)]
pub(crate) struct ExtractReport {
    /// 名前が危険なため、または読めないため展開しなかったエントリの数。
    pub skipped: usize,
}

/// `zip_path` を `dest` に展開する。
///
/// - 展開先の外を指す名前（`..`、絶対パス、ドライブ名）や、Windows で扱えない名前のエントリは展開しない
/// - シンボリックリンクと、パスワード付きのエントリは展開しない
/// - エントリ数と、実際に書き出した合計サイズが上限を超えたらエラーにする
/// - ファイル名は UTF-8 として読めればそのまま、読めなければ Shift_JIS として読む
pub(crate) fn extract_zip(zip_path: &Path, dest: &Path, limits: &Limits) -> Result<ExtractReport, OpenError> {
    let file = File::open(zip_path).map_err(OpenError::from_io)?;
    let mut archive = ZipArchive::new(file).map_err(|e| OpenError::InvalidZip(zip_error_message(&e)))?;
    if archive.len() > limits.max_entries {
        return Err(OpenError::TooLarge(format!("ファイル数が {} を超えています", limits.max_entries)));
    }

    let mut report = ExtractReport::default();
    let mut written: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = match archive.by_index(index) {
            Ok(entry) => entry,
            Err(ZipError::UnsupportedArchive(_) | ZipError::InvalidPassword) => {
                report.skipped += 1;
                continue;
            }
            Err(e) => return Err(OpenError::InvalidZip(zip_error_message(&e))),
        };
        let Some(parts) = safe_components(&decode_name(entry.name_raw())) else {
            report.skipped += 1;
            continue;
        };
        let target: PathBuf = parts.iter().fold(dest.to_path_buf(), |p, c| p.join(c));
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(OpenError::from_io)?;
            continue;
        }
        if entry.is_symlink() {
            report.skipped += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(OpenError::from_io)?;
        }
        let remaining = limits.max_total_bytes.saturating_sub(written);
        let mut out = File::create(&target).map_err(OpenError::from_io)?;
        let copied = match io::copy(&mut (&mut entry).take(remaining + 1), &mut out) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                // 壊れたエントリ（CRC 不一致など）。作りかけのファイルは消す。
                drop(out);
                let _ = fs::remove_file(&target);
                report.skipped += 1;
                continue;
            }
            Err(e) => return Err(OpenError::from_io(e)),
        };
        out.flush().map_err(OpenError::from_io)?;
        written += copied;
        if written > limits.max_total_bytes {
            return Err(OpenError::TooLarge(format!(
                "展開後の合計サイズが {} MB を超えています",
                limits.max_total_bytes / 1024 / 1024
            )));
        }
    }
    Ok(report)
}

fn zip_error_message(e: &ZipError) -> String {
    match e {
        ZipError::Io(io) => io.to_string(),
        other => other.to_string(),
    }
}

/// zip 内のファイル名のバイト列を文字列にする。UTF-8 として読めなければ Shift_JIS として読む。
pub(crate) fn decode_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode_without_bom_handling(raw).0.into_owned(),
    }
}

/// zip 内のパスを、展開先の下に収まる安全なパス要素の並びにする。危険なら `None`。
pub(crate) fn safe_components(name: &str) -> Option<Vec<String>> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for comp in name.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if !is_safe_component(comp) {
            return None;
        }
        parts.push(comp.to_string());
    }
    (!parts.is_empty()).then_some(parts)
}

fn is_safe_component(comp: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    if comp == ".." {
        return false;
    }
    // Windows は末尾のピリオドと空白を取り除くため、`.. ` が `..` として扱われうる。
    if comp.ends_with('.') || comp.ends_with(' ') {
        return false;
    }
    if comp.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
        return false;
    }
    let stem = comp.split('.').next().unwrap_or_default().to_ascii_lowercase();
    !RESERVED.contains(&stem.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_shift_jis_names() {
        assert_eq!(decode_name("返戻票.xml".as_bytes()), "返戻票.xml");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("返戻票.xml");
        assert_eq!(decode_name(&sjis), "返戻票.xml");
    }

    #[test]
    fn accepts_normal_paths() {
        assert_eq!(safe_components("202601/./henrei.xml"), Some(vec!["202601".to_string(), "henrei.xml".to_string()]));
        assert_eq!(safe_components("a\\b.xml"), Some(vec!["a".to_string(), "b.xml".to_string()]));
        assert_eq!(safe_components(".hidden"), Some(vec![".hidden".to_string()]));
    }

    #[test]
    fn rejects_traversal_and_windows_specials() {
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "/etc/passwd",
            "C:/Windows/evil.txt",
            "a/.. /b",
            "a/b.",
            "a/b ",
            "con.xml",
            "LPT1",
            "a/b:stream",
            "a/b?c",
            "",
            "./",
        ] {
            assert_eq!(safe_components(bad), None, "{bad}");
        }
    }
}
