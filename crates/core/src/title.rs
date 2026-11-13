//! 変換結果の HTML からタイトルを取り出す。

use crate::stylesheet::unescape;

/// HTML の最初の `<title>` の中身を、前後の空白を除き、連続する空白を 1 つにして返す。
/// `<title>` が無い、または中身が空なら `None`。
pub fn extract_title(html: &str) -> Option<String> {
    // ASCII だけを小文字にするので、バイト位置は元の文字列と一致する。
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    let open_end = loop {
        let start = from + lower[from..].find("<title")?;
        let next = lower.as_bytes().get(start + 6).copied();
        if matches!(next, Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')) {
            break start + lower[start..].find('>')? + 1;
        }
        from = start + 6;
    };
    let close = open_end + lower[open_end..].find("</title")?;
    let text = unescape(&html[open_end..close]);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title_case_insensitively() {
        let html = "<HTML><HEAD><TITLE>日本年金機構からのお知らせ</TITLE></HEAD></HTML>";
        assert_eq!(extract_title(html).as_deref(), Some("日本年金機構からのお知らせ"));
    }

    #[test]
    fn collapses_whitespace_and_unescapes() {
        let html = "<title>\n\t\t届書（申請書等）の\n\t\t補正 &amp; お願い\n</title>";
        assert_eq!(extract_title(html).as_deref(), Some("届書（申請書等）の 補正 & お願い"));
    }

    #[test]
    fn handles_attributes_and_ignores_similar_tags() {
        assert_eq!(extract_title("<titlebar>x</titlebar><title lang=\"ja\">本</title>").as_deref(), Some("本"));
    }

    #[test]
    fn returns_none_when_missing_or_empty() {
        assert_eq!(extract_title("<html><body>x</body></html>"), None);
        assert_eq!(extract_title("<title>  </title>"), None);
        assert_eq!(extract_title("<title>unterminated"), None);
    }
}
