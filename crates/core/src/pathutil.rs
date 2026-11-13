//! パスと相対 URL の扱い。

use std::path::Path;

/// 相対 URL（`href`）を、`base`（ルートからのフォルダの並び）を基準に解決する。
///
/// 戻り値はルートからのパス要素の並び。次の場合は `None` を返す。
/// - 空、または `#` で始まる（文書内の位置や埋め込みスタイルシートを指す）
/// - スキームを持つ（`https:`、`file:`、`C:` など。絶対 URL や絶対パス）
/// - `/` で始まる
/// - `..` でルートの外に出る
pub(crate) fn resolve_relative_href(base: &[String], href: &str) -> Option<Vec<String>> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') || has_scheme(href) {
        return None;
    }
    let path = href.split(['?', '#']).next().unwrap_or_default();
    let path = percent_decode(path).replace('\\', "/");
    if path.starts_with('/') {
        return None;
    }
    let mut parts = base.to_vec();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name.to_string()),
        }
    }
    if parts.is_empty() { None } else { Some(parts) }
}

/// `scheme:` の形で始まるかどうか（`C:` のようなドライブ名も含む）。
pub(crate) fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `%XX` を元のバイトに戻す。UTF-8 として読めない部分は置換文字になる。
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `root` から `path` までのパス要素の並び。`path` が `root` の下に無ければ `None`。
pub(crate) fn rel_components(root: &Path, path: &Path) -> Option<Vec<String>> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.iter().map(|c| c.to_string_lossy().into_owned()).collect())
}

/// パス要素の並び同士を比べる。Windows のファイル名と同じく、大文字・小文字は区別しない。
pub(crate) fn same_components(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_lowercase() == y.to_lowercase())
}

/// 2 つのパスが同じファイルを指すか。区切り文字（`/` と `\`）の違いと、大文字・小文字の違いは無視する。
pub fn same_path(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

/// パスの比較用の文字列（区切りを `/` にそろえ、小文字にしたもの）。
pub(crate) fn path_key(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_sibling_and_subfolder() {
        let base = v(&["202601"]);
        assert_eq!(resolve_relative_href(&base, "kagami.xsl"), Some(v(&["202601", "kagami.xsl"])));
        assert_eq!(resolve_relative_href(&base, "./sub/a.xsl"), Some(v(&["202601", "sub", "a.xsl"])));
        assert_eq!(resolve_relative_href(&base, "../a.xsl"), Some(v(&["a.xsl"])));
        assert_eq!(resolve_relative_href(&base, "sub\\a.xsl"), Some(v(&["202601", "sub", "a.xsl"])));
    }

    #[test]
    fn strips_query_and_fragment_and_decodes() {
        let base = v(&[]);
        assert_eq!(resolve_relative_href(&base, "a%20b.xsl?x=1#top"), Some(v(&["a b.xsl"])));
        assert_eq!(resolve_relative_href(&base, "%E8%BF%94%E6%88%BB.xml"), Some(v(&["返戻.xml"])));
    }

    #[test]
    fn rejects_outside_absolute_and_fragment() {
        let base = v(&["d"]);
        assert_eq!(resolve_relative_href(&base, "../../a.xsl"), None);
        assert_eq!(resolve_relative_href(&base, "https://example.com/a.xsl"), None);
        assert_eq!(resolve_relative_href(&base, "file:///C:/a.xsl"), None);
        assert_eq!(resolve_relative_href(&base, "C:\\a.xsl"), None);
        assert_eq!(resolve_relative_href(&base, "/a.xsl"), None);
        assert_eq!(resolve_relative_href(&base, "#style"), None);
        assert_eq!(resolve_relative_href(&base, "  "), None);
        assert_eq!(resolve_relative_href(&[], ".."), None);
    }

    #[test]
    fn detects_schemes() {
        assert!(has_scheme("http://x"));
        assert!(has_scheme("C:\\x"));
        assert!(has_scheme("javascript:alert(1)"));
        assert!(!has_scheme("a/b:c"));
        assert!(!has_scheme("kagami.xsl"));
        assert!(!has_scheme("1x:y"));
    }

    #[test]
    fn percent_decode_keeps_broken_sequences() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    #[test]
    fn same_path_ignores_case_and_separators() {
        assert!(same_path(Path::new("a/B/henrei.XML"), Path::new("a/b/henrei.xml")));
        assert!(!same_path(Path::new("a/b"), Path::new("a/b/c")));
        #[cfg(windows)]
        assert!(same_path(Path::new("C:\\x\\docs/henrei.xml"), Path::new("c:\\X\\docs\\henrei.xml")));
    }

    #[test]
    fn compares_case_insensitively() {
        assert!(same_components(&v(&["A", "Henrei.XML"]), &v(&["a", "henrei.xml"])));
        assert!(!same_components(&v(&["a"]), &v(&["a", "b"])));
    }
}
