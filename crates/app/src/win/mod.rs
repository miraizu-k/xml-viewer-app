//! Windows 版の本体: ウィンドウ、WebView2、画面との通信。
//!
//! 画面（`ui/` の HTML / CSS / JS）は exe に埋め込み、独自スキーム `app`（`http://app.localhost/`）で配る。
//! 画面からの呼び出しは `POST /api/{コマンド}` で受け、別スレッドで [`Api`] に処理させて JSON を返す。
//! ファイルのドロップなど、Rust 側から画面へ知らせることは `evaluate_script` で伝える。

mod com;
mod dialog;
mod integration;
mod shell;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowExtWindows;
use tao::window::WindowBuilder;
use wry::http::{Method, Request, Response, StatusCode};
use wry::{
    DragDropEvent, NewWindowResponse, RequestAsyncResponder, WebContext, WebViewBuilder, WebViewBuilderExtWindows,
};
use xml_viewer_core::history::RecentStore;
use xml_viewer_core::temp::sweep_stale;
use xml_viewer_core::xslt::Msxml;

use crate::api::{Api, IntegrationItem, Platform};

const APP_URL: &str = "http://app.localhost/";

/// 画面のページに付ける CSP。表示する文書（Blob の iframe）にも引き継がれる。
/// 文書のスクリプトは iframe の sandbox で止め、外部への通信はここで止める。
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data: blob:; font-src 'self' data:; frame-src blob:; connect-src 'self'; \
                   base-uri 'none'; form-action 'none'; object-src 'none'";

const INDEX_HTML: &str = include_str!("../../ui/index.html");
const APP_CSS: &str = include_str!("../../ui/app.css");
const APP_JS: &str = include_str!("../../ui/app.js");

/// ウィンドウのイベントループへ送る知らせ。
enum UserEvent {
    /// 画面に知らせる JavaScript。
    Script(String),
    /// ウィンドウのタイトルを変える。
    Title(String),
}

/// 保存先などのパス。
struct AppPaths {
    recent_file: PathBuf,
    webview_data: PathBuf,
    temp_root: PathBuf,
}

impl AppPaths {
    fn detect() -> Self {
        let env_dir = |name: &str| std::env::var_os(name).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        Self {
            recent_file: env_dir("APPDATA").join("xml-viewer").join("recent.json"),
            webview_data: env_dir("LOCALAPPDATA").join("xml-viewer").join("webview"),
            temp_root: std::env::temp_dir().join("xml-viewer"),
        }
    }
}

/// Windows の機能。
struct WinPlatform {
    owner: isize,
    integration: integration::Integration,
}

impl Platform for WinPlatform {
    fn open_file(&self, path: &Path) -> Result<(), String> {
        shell::open_file(path)
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        shell::open_url(url)
    }

    fn pick_files(&self) -> Result<Vec<PathBuf>, String> {
        dialog::pick_files(self.owner)
    }

    fn integration_status(&self) -> Result<Vec<IntegrationItem>, String> {
        Ok(self.integration.status())
    }

    fn integration_set(&self, key: &str, enabled: bool) -> Result<(), String> {
        self.integration.set(key, enabled)
    }
}

/// アプリを起動する。ウィンドウが閉じるまで戻らない。
pub fn run() {
    if let Err(message) = run_inner() {
        dialog::error_box(&message);
    }
}

fn run_inner() -> Result<(), String> {
    if wry::webview_version().is_err() {
        return Err("WebView2 ランタイムが見つからないため、起動できません。\n\
                    Microsoft のサイトから「WebView2 ランタイム」をインストールしてください。"
            .to_string());
    }
    let paths = AppPaths::detect();
    // 相対パスは、起動したときの作業フォルダを基準に絶対パスにする（履歴から開き直せるように）。
    let startup_paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(|arg| std::path::absolute(&arg).unwrap_or_else(|_| PathBuf::from(arg)))
        .collect();

    // 前回までの一時フォルダの残りを消す（ほかのウィンドウが使っているものは残る）。
    let temp_root = paths.temp_root.clone();
    std::thread::spawn(move || sweep_stale(&temp_root));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("XML Viewer")
        .with_inner_size(LogicalSize::new(1180.0, 820.0))
        .with_min_inner_size(LogicalSize::new(760.0, 480.0))
        .build(&event_loop)
        .map_err(|e| format!("ウィンドウを作れませんでした（{e}）"))?;

    let platform = WinPlatform { owner: window.hwnd(), integration: integration::Integration::current() };
    let api = Arc::new(Api::new(
        paths.temp_root.clone(),
        RecentStore::new(paths.recent_file.clone()),
        Box::new(Msxml),
        Box::new(platform),
        startup_paths,
    ));

    let mut web_context = WebContext::new(Some(paths.webview_data.clone()));
    let protocol_api = api.clone();
    let drop_proxy = event_loop.create_proxy();
    let title_proxy = event_loop.create_proxy();
    let webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_asynchronous_custom_protocol("app".into(), move |_id, request, responder| {
            handle_request(&protocol_api, request, responder);
        })
        .with_url(APP_URL)
        .with_drag_drop_handler(move |event| {
            forward_drag_drop(&drop_proxy, event);
            true
        })
        .with_document_title_changed_handler(move |title| {
            let _ = title_proxy.send_event(UserEvent::Title(title));
        })
        // 画面のページから、ほかのページへ移らないようにする。
        .with_navigation_handler(|url| url.starts_with(APP_URL))
        .with_new_window_req_handler(|_url, _features| NewWindowResponse::Deny)
        // 標準の右クリックメニュー（「戻る」「更新」「印刷」など、ウィンドウ全体に効く項目）は出さない。
        .with_default_context_menus(false)
        // 標準の拡大・縮小（Ctrl + ホイールなど）は画面全体に効くので使わない。文書だけを画面側（ui/app.js）で拡大する。
        .with_hotkeys_zoom(false)
        .with_devtools(cfg!(debug_assertions))
        .with_background_color((255, 255, 255, 255))
        .build(&window)
        .map_err(|e| format!("WebView2 を初期化できませんでした（{e}）"))?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Script(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::Title(title)) => {
                window.set_title(if title.is_empty() { "XML Viewer" } else { &title });
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                api.shutdown();
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => api.shutdown(),
            _ => {}
        }
        // web_context は WebView より長く生きている必要があるので、ループに持たせておく。
        let _ = &web_context;
    });
}

/// ファイルのドラッグを画面に知らせる。
fn forward_drag_drop(proxy: &EventLoopProxy<UserEvent>, event: DragDropEvent) {
    let js = match event {
        DragDropEvent::Enter { .. } => "window.__xmlv && window.__xmlv.drag(true)".to_string(),
        DragDropEvent::Leave => "window.__xmlv && window.__xmlv.drag(false)".to_string(),
        DragDropEvent::Drop { paths, .. } => {
            let list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let json = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
            format!("window.__xmlv && window.__xmlv.drop({json})")
        }
        _ => return,
    };
    let _ = proxy.send_event(UserEvent::Script(js));
}

/// 独自スキームへのリクエストに応える。
fn handle_request(api: &Arc<Api>, request: Request<Vec<u8>>, responder: RequestAsyncResponder) {
    let path = request.uri().path().to_string();
    if request.method() == Method::GET {
        let response = match path.as_str() {
            "/" | "/index.html" => asset(INDEX_HTML, "text/html; charset=utf-8", true),
            "/app.css" => asset(APP_CSS, "text/css; charset=utf-8", false),
            "/app.js" => asset(APP_JS, "text/javascript; charset=utf-8", false),
            _ => status(StatusCode::NOT_FOUND),
        };
        responder.respond(response);
        return;
    }
    let Some(command) = path.strip_prefix("/api/").map(str::to_string) else {
        responder.respond(status(StatusCode::NOT_FOUND));
        return;
    };
    // 文書の中の画像やリンクからは独自ヘッダーを付けられないので、画面のスクリプトからの呼び出しだけが通る。
    let from_app = request.headers().get("x-xmlv").is_some_and(|v| v == "1");
    if request.method() != Method::POST || !from_app {
        responder.respond(status(StatusCode::FORBIDDEN));
        return;
    }
    let api = api.clone();
    let body = request.into_body();
    std::thread::spawn(move || {
        let payload = match api.handle(&command, &body) {
            Ok(data) => serde_json::json!({ "ok": true, "data": data }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };
        let bytes = serde_json::to_vec(&payload).unwrap_or_default();
        responder.respond(
            Response::builder()
                .header("Content-Type", "application/json; charset=utf-8")
                .header("Cache-Control", "no-store")
                .body(Cow::Owned(bytes))
                .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR)),
        );
    });
}

fn asset(body: &'static str, content_type: &str, with_csp: bool) -> Response<Cow<'static, [u8]>> {
    let mut builder = Response::builder().header("Content-Type", content_type).header("Cache-Control", "no-store");
    if with_csp {
        builder = builder.header("Content-Security-Policy", CSP);
    }
    builder.body(Cow::Borrowed(body.as_bytes())).unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR))
}

fn status(code: StatusCode) -> Response<Cow<'static, [u8]>> {
    Response::builder().status(code).body(Cow::Borrowed(&[][..])).expect("固定のレスポンス")
}
