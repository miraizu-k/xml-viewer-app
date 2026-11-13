//! XML の `<?xml-stylesheet ?>` を読み、使う XSL のファイルを決める。

use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use encoding_rs::Encoding;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::pathutil::{rel_components, resolve_relative_href};

/// スタイルシートの指定を探すときに読む、ファイル先頭の大きさ。
const PROLOG_LIMIT: u64 = 256 * 1024;

/// XML ファイルが指す XSL を、対象フォルダ `root` の中から探す。
///
/// 次の場合は `None` を返す（ページとして扱わない）。
/// - XML として読めない、または `xml-stylesheet` の指定が無い
/// - `href` が絶対 URL、`root` の外を指すパス、`#id` 形式（埋め込みスタイルシート）
/// - 指す先のファイルが無い
pub fn find_stylesheet(xml_path: &Path, root: &Path) -> Option<PathBuf> {
    let mut head = Vec::new();
    File::open(xml_path).ok()?.take(PROLOG_LIMIT).read_to_end(&mut head).ok()?;
    let href = stylesheet_href(&decode_xml(&head))?;
    let base = rel_components(root, xml_path.parent()?)?;
    let parts = resolve_relative_href(&base, &href)?;
    let path = parts.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    path.is_file().then_some(path)
}

/// XML のバイト列を文字列にする。
///
/// BOM があればそれに従い、無ければ XML 宣言の `encoding` に従う。どちらも無ければ UTF-8。
pub fn decode_xml(bytes: &[u8]) -> Cow<'_, str> {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom_len..]).0;
    }
    let encoding = declared_encoding(bytes).unwrap_or(encoding_rs::UTF_8);
    encoding.decode_without_bom_handling(bytes).0
}

/// XML 宣言の `encoding` を読む。宣言が無い、または知らない名前なら `None`。
fn declared_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(512)];
    if !head.starts_with(b"<?xml") {
        return None;
    }
    let end = head.windows(2).position(|w| w == b"?>")?;
    let decl = std::str::from_utf8(&head[..end]).ok()?;
    let value = pseudo_attributes(decl.trim_start_matches("<?xml")).into_iter().find(|(name, _)| name == "encoding")?.1;
    let encoding = Encoding::for_label(value.as_bytes())?;
    // UTF-16 と宣言していても、BOM が無く ASCII 互換で書かれていれば UTF-8 として読むほかない。
    (encoding != encoding_rs::UTF_16LE && encoding != encoding_rs::UTF_16BE).then_some(encoding)
}

/// 文書のルート要素より前にある `xml-stylesheet` のうち、XSL を指す最初のものの `href`。
pub fn stylesheet_href(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::PI(pi)) if pi.target() == "xml-stylesheet" => {
                let attrs = pseudo_attributes(pi.content());
                let get = |key: &str| attrs.iter().find(|(n, _)| n == key).map(|(_, v)| v.as_str());
                if get("alternate").is_some_and(|v| v.eq_ignore_ascii_case("yes")) {
                    continue;
                }
                let Some(href) = get("href") else { continue };
                if is_xsl_type(get("type"), href) {
                    return Some(href.to_string());
                }
            }
            Ok(Event::Start(_) | Event::Empty(_) | Event::Eof) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// `type` の値（無ければ `href` の拡張子）から、XSL を指しているかを判断する。
fn is_xsl_type(type_attr: Option<&str>, href: &str) -> bool {
    match type_attr {
        Some(t) => {
            let mime = t.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            matches!(mime.as_str(), "text/xsl" | "application/xslt+xml" | "text/xml" | "application/xml")
        }
        None => {
            let lower = href.to_ascii_lowercase();
            let path = lower.split(['?', '#']).next().unwrap_or_default();
            path.ends_with(".xsl") || path.ends_with(".xslt")
        }
    }
}

/// 処理命令の中身（`type="text/xsl" href="a.xsl"`）を、名前と値の組に分ける。
fn pseudo_attributes(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = content;
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(end) = after[1..].find(quote) else { break };
        out.push((name, unescape(&after[1..1 + end])));
        rest = &after[end + 2..];
    }
    out
}

/// 文字参照と定義済みの実体参照を元の文字に戻す。
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|&i| i <= 12) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_href_regardless_of_attribute_order() {
        let a = "<?xml version=\"1.0\"?>\n<?xml-stylesheet type=\"text/xsl\" href=\"kagami.xsl\"?>\n<DOC/>";
        let b = "<?xml version=\"1.0\"?><?xml-stylesheet href='AS1.xsl' type='text/xsl'?><DataRoot/>";
        assert_eq!(stylesheet_href(a).as_deref(), Some("kagami.xsl"));
        assert_eq!(stylesheet_href(b).as_deref(), Some("AS1.xsl"));
    }

    #[test]
    fn skips_css_alternate_and_comments() {
        let xml = r#"<?xml version="1.0"?>
<!-- <?xml-stylesheet type="text/xsl" href="commented.xsl"?> -->
<?xml-stylesheet type="text/css" href="a.css"?>
<?xml-stylesheet type="text/xsl" href="alt.xsl" alternate="yes"?>
<?xml-stylesheet type="text/xsl" href="main.xsl"?>
<root/>"#;
        assert_eq!(stylesheet_href(xml).as_deref(), Some("main.xsl"));
    }

    #[test]
    fn ignores_instructions_after_root_and_missing_ones() {
        assert_eq!(stylesheet_href("<root/><?xml-stylesheet type=\"text/xsl\" href=\"a.xsl\"?>"), None);
        assert_eq!(stylesheet_href("<?xml version=\"1.0\"?><root/>"), None);
        assert_eq!(stylesheet_href("not xml"), None);
    }

    #[test]
    fn accepts_missing_type_only_for_xsl_extension() {
        assert_eq!(stylesheet_href("<?xml-stylesheet href=\"a.xsl\"?><r/>").as_deref(), Some("a.xsl"));
        assert_eq!(stylesheet_href("<?xml-stylesheet href=\"a.css\"?><r/>"), None);
    }

    #[test]
    fn handles_doctype_with_internal_subset() {
        let xml = "<?xml-stylesheet type=\"text/xsl\" href=\"a.xsl\"?><!DOCTYPE r [<!ENTITY x \"y>\">]><r/>";
        assert_eq!(stylesheet_href(xml).as_deref(), Some("a.xsl"));
    }

    #[test]
    fn unescapes_href() {
        let xml = "<?xml-stylesheet type=\"text/xsl\" href=\"a&amp;b&#x20;c.xsl\"?><r/>";
        assert_eq!(stylesheet_href(xml).as_deref(), Some("a&b c.xsl"));
    }

    #[test]
    fn decodes_by_bom_and_declaration() {
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("<?xml version=\"1.0\" encoding=\"Shift_JIS\"?><r>返戻</r>");
        assert!(decode_xml(&sjis).contains("返戻"));
        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice("<r>鑑</r>".as_bytes());
        assert_eq!(decode_xml(&utf8_bom), "<r>鑑</r>");
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("<r>様式</r>".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        assert_eq!(decode_xml(&utf16), "<r>様式</r>");
        assert_eq!(decode_xml("<r>なし</r>".as_bytes()), "<r>なし</r>");
    }

    #[test]
    fn unescape_leaves_unknown_entities() {
        assert_eq!(unescape("a &unknown; b &amp; c & d"), "a &unknown; b & c & d");
        assert_eq!(unescape("&#12354;&#x3044;"), "あい");
    }
}
