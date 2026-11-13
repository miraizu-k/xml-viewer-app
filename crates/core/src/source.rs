//! zip・XML・フォルダを開き、表示できるページとその他のファイルに分ける。
//!
//! 開くものが何であっても、内部では「あるフォルダ（対象フォルダ）の中のファイル群」として扱う。
//!
//! | 開いたもの | 対象フォルダ | 見る深さ |
//! |---|---|---|
//! | zip | 一時フォルダに展開した先 | 下の階層も見る |
//! | XML | その XML がある元のフォルダ | 同じ階層だけ |
//! | フォルダ | そのフォルダ | 下の階層も見る |

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::extract::{ExtractReport, extract_zip};
use crate::pathutil::{path_key, rel_components, same_path};
use crate::stylesheet::find_stylesheet;
use crate::temp::TempSession;
use crate::title::extract_title;
use crate::xslt::{TransformError, Transformer};

/// 開くときの上限。
#[derive(Debug, Clone)]
pub struct Limits {
    /// zip を展開したときの合計サイズ（バイト）。
    pub max_total_bytes: u64,
    /// zip のエントリ数。
    pub max_entries: usize,
    /// 一覧に入れるページの数。超えた分は一覧に入れない。
    pub max_pages: usize,
    /// 一覧に入れるその他のファイルの数。
    pub max_others: usize,
    /// zip とフォルダで、下の階層を見る深さ。
    pub max_depth: usize,
    /// ページかどうかを調べる XML の大きさ。これより大きい XML はその他のファイルにする。
    pub max_xml_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_total_bytes: 500 * 1024 * 1024,
            max_entries: 5_000,
            max_pages: 200,
            max_others: 2_000,
            max_depth: 5,
            max_xml_bytes: 64 * 1024 * 1024,
        }
    }
}

/// 開いたものの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Zip,
    Xml,
    Folder,
}

/// 開けなかった理由。
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("ファイルが見つかりません")]
    NotFound,
    #[error("ファイルを読めません（{0}）")]
    Io(io::Error),
    #[error("zip として読めません（{0}）")]
    InvalidZip(String),
    #[error("大きすぎるため開きません（{0}）")]
    TooLarge(String),
    #[error("zip、XML、フォルダのどれでもありません")]
    Unsupported,
}

impl OpenError {
    pub(crate) fn from_io(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::NotFound { Self::NotFound } else { Self::Io(e) }
    }

    /// 画面で出し分けるための種類名。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Io(_) => "io",
            Self::InvalidZip(_) => "invalid_zip",
            Self::TooLarge(_) => "too_large",
            Self::Unsupported => "unsupported",
        }
    }
}

/// 表示できるページ（XML と XSL の組）。
#[derive(Debug)]
pub struct Page {
    /// 対象フォルダからの相対パスの要素。
    pub rel: Vec<String>,
    pub xml: PathBuf,
    pub xsl: PathBuf,
    /// 変換結果の `<title>`。取れなければファイル名。
    pub title: String,
    /// 変換結果の HTML。失敗したらその理由。
    pub html: Result<String, TransformError>,
}

/// ページでないファイル（PDF、CSV、XSL の指定が無い XML など）。
#[derive(Debug)]
pub struct OtherFile {
    pub rel: Vec<String>,
    pub path: PathBuf,
}

/// 開いた結果。
#[derive(Debug)]
pub struct Source {
    /// 開いたパス。
    pub input: PathBuf,
    pub kind: InputKind,
    /// 対象フォルダ。
    pub root: PathBuf,
    pub pages: Vec<Page>,
    /// その他のファイル。リンクの行き先にもなるので、一覧に出さない場合も持っておく。
    pub others: Vec<OtherFile>,
    /// その他のファイルを一覧に出すか。XML を直接開いたときは出さない
    /// （ダウンロードフォルダなど、無関係なファイルが多い場所で一覧が埋まるのを避けるため）。
    pub list_others: bool,
    /// 最初に選ぶページ。XML を直接開いたときは、その XML。
    pub initial_page: Option<usize>,
    /// ページ数の上限を超えて、一覧に入れなかったページがある。
    pub truncated: bool,
    /// zip の中で、名前が危険などの理由で展開しなかったエントリの数。
    pub skipped_entries: usize,
    /// zip を展開したフォルダ。閉じるときに消す。
    extracted: Option<PathBuf>,
}

impl Source {
    /// 開いたもの（zip・XML・フォルダ）の表示名。
    pub fn name(&self) -> String {
        self.input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.input.to_string_lossy().into_owned())
    }

    /// 後片付けをする。zip を展開したフォルダを消す。
    pub fn close(&mut self) {
        if let Some(dir) = self.extracted.take() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.close();
    }
}

/// パスの種類を見分ける。
pub fn detect_kind(path: &Path) -> Result<InputKind, OpenError> {
    let meta = fs::metadata(path).map_err(OpenError::from_io)?;
    if meta.is_dir() {
        return Ok(InputKind::Folder);
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut head = [0u8; 64];
    let n = fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(OpenError::from_io)?;
    let head = &head[..n];
    if ext == "zip" || head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Ok(InputKind::Zip);
    }
    if ext == "xml" || looks_like_xml(head) {
        return Ok(InputKind::Xml);
    }
    Err(OpenError::Unsupported)
}

fn looks_like_xml(head: &[u8]) -> bool {
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    head.starts_with(b"<?xml") || head.starts_with(&[0xFF, 0xFE, b'<', 0]) || head.starts_with(&[0xFE, 0xFF, 0, b'<'])
}

/// パスを開いて、ページとその他のファイルに分け、各ページを変換する。
///
/// zip は `temp` の下に展開する。`temp` は zip を開くときだけ使う。
pub fn open(
    path: &Path,
    temp: &dyn Fn() -> io::Result<PathBuf>,
    transformer: &dyn Transformer,
    limits: &Limits,
) -> Result<Source, OpenError> {
    let kind = detect_kind(path)?;
    let mut extracted = None;
    let mut skipped_entries = 0;
    let (root, depth) = match kind {
        InputKind::Zip => {
            let dest = temp().map_err(OpenError::Io)?;
            // 展開に失敗したら、作りかけのフォルダを消す。
            let report: ExtractReport = match extract_zip(path, &dest, limits) {
                Ok(r) => r,
                Err(e) => {
                    let _ = fs::remove_dir_all(&dest);
                    return Err(e);
                }
            };
            skipped_entries = report.skipped;
            extracted = Some(dest.clone());
            (dest, limits.max_depth)
        }
        InputKind::Folder => (path.to_path_buf(), limits.max_depth),
        InputKind::Xml => (path.parent().map(Path::to_path_buf).unwrap_or_default(), 0),
    };

    let files = list_files(&root, depth);
    let mut source = classify(path, kind, &root, files, transformer, limits);
    source.skipped_entries = skipped_entries;
    source.extracted = extracted;
    Ok(source)
}

/// 対象フォルダのファイルを、相対パスの順（大文字・小文字を区別しない）に並べて返す。
fn list_files(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, 0, max_depth, &mut out);
    out.sort_by_cached_key(|p| {
        p.strip_prefix(root).unwrap_or(p).iter().map(|c| c.to_string_lossy().to_lowercase()).collect::<Vec<_>>()
    });
    out
}

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_ignored_name(&name) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else { continue };
        // リンクはたどらない（ループや、対象フォルダの外へ出るのを避ける）。
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if depth < max_depth {
                walk(&entry.path(), depth + 1, max_depth, out);
            }
        } else if file_type.is_file() {
            out.push(entry.path());
        }
    }
}

fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.') || lower == "__macosx" || lower == "thumbs.db" || lower == "desktop.ini"
}

fn classify(
    input: &Path,
    kind: InputKind,
    root: &Path,
    files: Vec<PathBuf>,
    transformer: &dyn Transformer,
    limits: &Limits,
) -> Source {
    // XML のうち、XSL の指定があり、その XSL が対象フォルダ内にあるものがページ。
    let mut candidates = Vec::new();
    let mut referenced_xsl = HashSet::new();
    for file in &files {
        if !has_extension(file, "xml") || fs::metadata(file).map(|m| m.len()).unwrap_or(u64::MAX) > limits.max_xml_bytes
        {
            continue;
        }
        if let Some(xsl) = find_stylesheet(file, root) {
            referenced_xsl.insert(path_key(&xsl));
            candidates.push((file.clone(), xsl));
        }
    }
    let page_set: HashSet<String> = candidates.iter().map(|(x, _)| path_key(x)).collect();

    let truncated = candidates.len() > limits.max_pages;
    // XML を直接開いたときは、その XML が上限で切られないように先頭に寄せて数える。
    if kind == InputKind::Xml
        && let Some(pos) = candidates.iter().position(|(x, _)| same_path(x, input))
    {
        let opened = candidates.remove(pos);
        candidates.truncate(limits.max_pages.saturating_sub(1));
        candidates.push(opened);
        candidates.sort_by_cached_key(|(x, _)| path_key(x));
    }
    candidates.truncate(limits.max_pages);

    let pages: Vec<Page> = candidates
        .into_iter()
        .map(|(xml, xsl)| {
            let rel = rel_components(root, &xml).unwrap_or_default();
            let html = transformer.transform(&xml, &xsl);
            let file_name = rel.last().cloned().unwrap_or_default();
            let title = html.as_ref().ok().and_then(|h| extract_title(h)).unwrap_or(file_name);
            Page { rel, xml, xsl, title, html }
        })
        .collect();

    let others: Vec<OtherFile> = files
        .iter()
        .filter(|f| {
            let key = path_key(f);
            !page_set.contains(&key) && !referenced_xsl.contains(&key)
        })
        .take(limits.max_others)
        .map(|f| OtherFile { rel: rel_components(root, f).unwrap_or_default(), path: f.clone() })
        .collect();

    let initial_page = match kind {
        InputKind::Xml => pages.iter().position(|p| same_path(&p.xml, input)),
        _ if pages.is_empty() => None,
        _ => Some(0),
    };

    Source {
        input: input.to_path_buf(),
        kind,
        root: root.to_path_buf(),
        pages,
        others,
        list_others: kind != InputKind::Xml,
        initial_page,
        truncated,
        skipped_entries: 0,
        extracted: None,
    }
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// [`TempSession`] から展開先を作る関数を用意する。
pub fn temp_dirs(session: &TempSession) -> impl Fn() -> io::Result<PathBuf> + '_ {
    move || session.new_extract_dir()
}
