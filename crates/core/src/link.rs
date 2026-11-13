//! 文書の中のリンクの行き先を決める。
//!
//! | リンク先 | 行き先 |
//! |---|---|
//! | 同じ対象フォルダ内のページ（例: `henrei.xml`） | そのページ |
//! | 同じ対象フォルダ内のその他のファイル | 既定のアプリで開く |
//! | `http://` / `https://` | 既定のブラウザで開く |
//! | それ以外 | 何もしない |

use crate::pathutil::{has_scheme, resolve_relative_href, same_components};
use crate::source::Source;

/// リンクの行き先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// ページ（`Source::pages` の添字）。
    Page(usize),
    /// その他のファイル（`Source::others` の添字）。
    Other(usize),
    /// 既定のブラウザで開く URL。
    External(String),
    /// 何もしない。
    None,
}

/// `from_page` のページの中にある `href` の行き先を決める。
pub fn resolve_link(source: &Source, from_page: usize, href: &str) -> LinkTarget {
    let href = href.trim();
    if has_scheme(href) {
        let lower = href.to_ascii_lowercase();
        return if lower.starts_with("http://") || lower.starts_with("https://") {
            LinkTarget::External(href.to_string())
        } else {
            LinkTarget::None
        };
    }
    let Some(page) = source.pages.get(from_page) else {
        return LinkTarget::None;
    };
    let base = &page.rel[..page.rel.len().saturating_sub(1)];
    let Some(target) = resolve_relative_href(base, href) else {
        return LinkTarget::None;
    };
    if let Some(i) = source.pages.iter().position(|p| same_components(&p.rel, &target)) {
        return LinkTarget::Page(i);
    }
    if let Some(i) = source.others.iter().position(|o| same_components(&o.rel, &target)) {
        return LinkTarget::Other(i);
    }
    LinkTarget::None
}
