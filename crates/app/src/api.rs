//! 画面からの呼び出しを処理する。
//!
//! 画面（JavaScript）は `POST /api/{コマンド}` に JSON を送り、JSON を受け取る。
//! ここは OS に依存しない。ファイルを既定のアプリで開く・ファイル選択ダイアログを出すなど、
//! OS の機能を使う部分は [`Platform`] として外から渡す。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use xml_viewer_core::history::RecentStore;
use xml_viewer_core::link::{LinkTarget, resolve_link};
use xml_viewer_core::source::{self, InputKind, Limits, Source};
use xml_viewer_core::temp::TempSession;
use xml_viewer_core::xslt::Transformer;

/// エクスプローラー連携の項目の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationState {
    /// 登録されていない。
    Off,
    /// 今の exe の場所で登録されている。
    On,
    /// 登録されているが、別の場所の exe を指している（exe を移動した）。
    Stale,
}

/// エクスプローラー連携の項目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrationItem {
    pub key: String,
    pub state: IntegrationState,
}

/// OS の機能。
pub trait Platform: Send + Sync {
    /// ファイルを既定のアプリで開く。
    fn open_file(&self, path: &Path) -> Result<(), String>;
    /// URL を既定のブラウザで開く。
    fn open_url(&self, url: &str) -> Result<(), String>;
    /// ファイル選択ダイアログを出す。取り消されたら空。
    fn pick_files(&self) -> Result<Vec<PathBuf>, String>;
    /// エクスプローラー連携の状態。
    fn integration_status(&self) -> Result<Vec<IntegrationItem>, String>;
    /// エクスプローラー連携を入れる・外す。
    fn integration_set(&self, key: &str, enabled: bool) -> Result<(), String>;
}

/// 呼び出しの失敗。画面には `{ ok: false, error: { kind, message } }` で返す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    pub kind: String,
    pub message: String,
}

impl ApiError {
    fn new(kind: &str, message: impl Into<String>) -> Self {
        Self { kind: kind.to_string(), message: message.into() }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new("bad_request", message)
    }
}

/// 呼び出しを処理するもの。ウィンドウ 1 つにつき 1 つ。
pub struct Api {
    state: Mutex<State>,
    temp: Mutex<Option<Arc<TempSession>>>,
    temp_root: PathBuf,
    recent: RecentStore,
    transformer: Box<dyn Transformer + Send>,
    platform: Box<dyn Platform>,
    limits: Limits,
}

#[derive(Default)]
struct State {
    /// 開いているもの。画面の一覧と同じく、新しいものが先頭。
    sources: Vec<(u32, Source)>,
    next_id: u32,
    /// 起動引数で渡されたパス。最初の `startup` で一度だけ返す。
    startup_paths: Option<Vec<PathBuf>>,
}

/// 開いたものを画面に渡す形。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceView {
    id: u32,
    name: String,
    input: String,
    kind: InputKind,
    pages: Vec<PageView>,
    others: Vec<OtherView>,
    list_others: bool,
    initial_page: Option<usize>,
    truncated: bool,
    skipped_entries: usize,
}

#[derive(Debug, Serialize)]
struct PageView {
    index: usize,
    title: String,
    file: String,
    folder: String,
    failed: bool,
}

#[derive(Debug, Serialize)]
struct OtherView {
    index: usize,
    file: String,
    folder: String,
}

fn split_rel(rel: &[String]) -> (String, String) {
    let file = rel.last().cloned().unwrap_or_default();
    let folder = rel[..rel.len().saturating_sub(1)].join("/");
    (file, folder)
}

impl SourceView {
    fn new(id: u32, s: &Source) -> Self {
        Self {
            id,
            name: s.name(),
            input: s.input.to_string_lossy().into_owned(),
            kind: s.kind,
            pages: s
                .pages
                .iter()
                .enumerate()
                .map(|(index, p)| {
                    let (file, folder) = split_rel(&p.rel);
                    PageView { index, title: p.title.clone(), file, folder, failed: p.html.is_err() }
                })
                .collect(),
            others: s
                .others
                .iter()
                .enumerate()
                .map(|(index, o)| {
                    let (file, folder) = split_rel(&o.rel);
                    OtherView { index, file, folder }
                })
                .collect(),
            list_others: s.list_others,
            initial_page: s.initial_page,
            truncated: s.truncated,
            skipped_entries: s.skipped_entries,
        }
    }
}

#[derive(Deserialize)]
struct PathsArgs {
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct SourceArgs {
    source: u32,
}

#[derive(Deserialize)]
struct PageArgs {
    source: u32,
    page: usize,
}

#[derive(Deserialize)]
struct LinkArgs {
    source: u32,
    page: usize,
    href: String,
}

#[derive(Deserialize)]
struct OtherArgs {
    source: u32,
    index: usize,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct IntegrationArgs {
    key: String,
    enabled: bool,
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ApiError> {
    let body = if body.is_empty() { b"{}".as_slice() } else { body };
    serde_json::from_slice(body).map_err(|e| ApiError::bad_request(format!("引数が正しくありません（{e}）")))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

impl Api {
    pub fn new(
        temp_root: PathBuf,
        recent: RecentStore,
        transformer: Box<dyn Transformer + Send>,
        platform: Box<dyn Platform>,
        startup_paths: Vec<PathBuf>,
    ) -> Self {
        Self {
            state: Mutex::new(State { startup_paths: Some(startup_paths), next_id: 1, ..State::default() }),
            temp: Mutex::new(None),
            temp_root,
            recent,
            transformer,
            platform,
            limits: Limits::default(),
        }
    }

    /// 上限を差し替える（テスト用）。
    #[cfg(test)]
    fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// コマンドを処理する。
    pub fn handle(&self, command: &str, body: &[u8]) -> Result<Value, ApiError> {
        match command {
            "startup" => Ok(self.startup()),
            "open" => self.open(parse::<PathsArgs>(body)?.paths),
            "page" => self.page(parse(body)?),
            "link" => self.link(parse(body)?),
            "openOther" => self.open_other(parse(body)?),
            "close" => self.close(parse::<SourceArgs>(body)?.source),
            "pick" => self.pick(),
            "recent" => Ok(json!({ "items": self.recent.load() })),
            "recentRemove" => {
                let path = parse::<PathArgs>(body)?.path;
                let items = self.recent.remove(Path::new(&path)).map_err(|e| ApiError::new("io", e.to_string()))?;
                Ok(json!({ "items": items }))
            }
            "recentClear" => {
                self.recent.clear().map_err(|e| ApiError::new("io", e.to_string()))?;
                Ok(json!({ "items": [] }))
            }
            "integration" => self.integration_status(),
            "integrationSet" => {
                let args: IntegrationArgs = parse(body)?;
                self.platform.integration_set(&args.key, args.enabled).map_err(|e| ApiError::new("integration", e))?;
                self.integration_status()
            }
            "integrationRepair" => {
                let items = self.platform.integration_status().map_err(|e| ApiError::new("integration", e))?;
                for item in items.iter().filter(|i| i.state == IntegrationState::Stale) {
                    self.platform.integration_set(&item.key, true).map_err(|e| ApiError::new("integration", e))?;
                }
                self.integration_status()
            }
            _ => Err(ApiError::new("unknown_command", format!("知らないコマンドです: {command}"))),
        }
    }

    fn startup(&self) -> Value {
        let paths = self.state().startup_paths.take().unwrap_or_default();
        json!({ "paths": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>() })
    }

    /// パスを順に開く。開けなかったものは理由を返し、ほかは続ける。
    fn open(&self, paths: Vec<String>) -> Result<Value, ApiError> {
        let results: Vec<Value> = paths.iter().map(|p| self.open_one(Path::new(p))).collect();
        Ok(json!({ "results": results }))
    }

    fn open_one(&self, path: &Path) -> Value {
        let path_text = path.to_string_lossy();
        // すでに開いているパスなら、選択を移すだけにする。
        if let Some(view) = {
            let state = self.state();
            state
                .sources
                .iter()
                .find(|(_, s)| xml_viewer_core::same_path(&s.input, path))
                .map(|(id, s)| SourceView::new(*id, s))
        } {
            let _ = self.recent.add(path, now_secs());
            return json!({ "path": path_text, "ok": true, "existing": true, "source": view });
        }

        let temp = || -> std::io::Result<PathBuf> { self.temp_session()?.new_extract_dir() };
        match source::open(path, &temp, self.transformer.as_ref(), &self.limits) {
            Ok(source) => {
                let _ = self.recent.add(path, now_secs());
                let mut state = self.state();
                let id = state.next_id;
                state.next_id += 1;
                let view = SourceView::new(id, &source);
                state.sources.insert(0, (id, source));
                json!({ "path": path_text, "ok": true, "existing": false, "source": view })
            }
            Err(e) => json!({
                "path": path_text,
                "ok": false,
                "error": { "kind": e.kind(), "message": e.to_string() },
            }),
        }
    }

    /// この起動用の一時フォルダ。最初に zip を開くときに作る。
    fn temp_session(&self) -> std::io::Result<Arc<TempSession>> {
        let mut temp = self.temp.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = temp.as_ref() {
            return Ok(session.clone());
        }
        let session = Arc::new(TempSession::create(&self.temp_root)?);
        *temp = Some(session.clone());
        Ok(session)
    }

    fn with_source<T>(&self, id: u32, f: impl FnOnce(&Source) -> Result<T, ApiError>) -> Result<T, ApiError> {
        let state = self.state();
        let (_, source) = state
            .sources
            .iter()
            .find(|(sid, _)| *sid == id)
            .ok_or_else(|| ApiError::new("not_open", "このファイルはもう閉じられています"))?;
        f(source)
    }

    fn page(&self, args: PageArgs) -> Result<Value, ApiError> {
        self.with_source(args.source, |s| {
            let page = s.pages.get(args.page).ok_or_else(|| ApiError::bad_request("ページがありません"))?;
            Ok(match &page.html {
                Ok(html) => json!({ "title": page.title, "html": html, "error": null }),
                Err(e) => json!({ "title": page.title, "html": null, "error": e }),
            })
        })
    }

    fn link(&self, args: LinkArgs) -> Result<Value, ApiError> {
        let target = self.with_source(args.source, |s| {
            Ok(match resolve_link(s, args.page, &args.href) {
                LinkTarget::Other(i) => (LinkTarget::Other(i), s.others.get(i).map(|o| o.path.clone())),
                other => (other, None),
            })
        })?;
        match target {
            (LinkTarget::Page(page), _) => Ok(json!({ "kind": "page", "page": page })),
            (LinkTarget::Other(_), Some(path)) => {
                self.platform.open_file(&path).map_err(|e| ApiError::new("open_failed", e))?;
                Ok(json!({ "kind": "opened" }))
            }
            (LinkTarget::External(url), _) => {
                self.platform.open_url(&url).map_err(|e| ApiError::new("open_failed", e))?;
                Ok(json!({ "kind": "opened" }))
            }
            _ => Ok(json!({ "kind": "none" })),
        }
    }

    fn open_other(&self, args: OtherArgs) -> Result<Value, ApiError> {
        let path = self.with_source(args.source, |s| {
            s.others
                .get(args.index)
                .map(|o| o.path.clone())
                .ok_or_else(|| ApiError::bad_request("ファイルがありません"))
        })?;
        self.platform.open_file(&path).map_err(|e| ApiError::new("open_failed", e))?;
        Ok(json!({}))
    }

    fn close(&self, id: u32) -> Result<Value, ApiError> {
        let removed = {
            let mut state = self.state();
            let pos = state.sources.iter().position(|(sid, _)| *sid == id);
            pos.map(|i| state.sources.remove(i))
        };
        // 一時フォルダの削除はロックの外で行う（Source を破棄すると消える）。
        drop(removed);
        Ok(json!({}))
    }

    fn pick(&self) -> Result<Value, ApiError> {
        let paths = self.platform.pick_files().map_err(|e| ApiError::new("dialog", e))?;
        Ok(json!({ "paths": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>() }))
    }

    fn integration_status(&self) -> Result<Value, ApiError> {
        let items = self.platform.integration_status().map_err(|e| ApiError::new("integration", e))?;
        Ok(json!({ "items": items }))
    }

    /// 終了時の後片付け。開いているものと、この起動用の一時フォルダを消す。
    pub fn shutdown(&self) {
        let sources = std::mem::take(&mut self.state().sources);
        drop(sources);
        let session = self.temp.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(session);
    }
}

#[cfg(test)]
mod tests;
