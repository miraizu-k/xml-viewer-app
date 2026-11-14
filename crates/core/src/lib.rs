//! xml-viewer のロジック。
//!
//! - [`source`]：zip・XML・フォルダを開き、表示できるページとその他のファイルに分ける
//! - [`xslt`]：XML と XSL から HTML を作る（Windows では MSXML 6 を使う）
//! - [`link`]：文書の中のリンクの行き先を決める
//! - [`temp`]：zip を展開する一時フォルダの管理
//! - [`history`]：最近開いたファイルの記録
//! - [`settings`]：アプリの設定
//!
//! 画面や OS の機能には依存しない。MSXML を使う [`xslt::Msxml`] だけが Windows 専用で、
//! それ以外の OS では「未対応」のエラーを返す。

pub mod history;
pub mod link;
pub mod settings;
pub mod source;
pub mod stylesheet;
pub mod temp;
pub mod title;
pub mod xslt;

mod extract;
mod fsutil;
mod pathutil;

pub use pathutil::same_path;
