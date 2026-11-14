//! 画面からの呼び出しのテスト。変換と OS の機能は仮のものを使う。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use xml_viewer_core::history::RecentStore;
use xml_viewer_core::xslt::{TransformError, Transformer};

use super::*;

struct Work(PathBuf);

impl Work {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("xmlv-api-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures").join(name)
}

fn zip_fixture(name: &str, prefix: &str, out: &Path) {
    let mut zip = zip::ZipWriter::new(fs::File::create(out).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    let mut entries: Vec<_> = fs::read_dir(fixture(name)).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        zip.start_file(format!("{prefix}/{}", path.file_name().unwrap().to_string_lossy()), opts).unwrap();
        zip.write_all(&fs::read(&path).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

struct FakeTransformer;

impl Transformer for FakeTransformer {
    fn transform(&self, xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        if xsl.ends_with("broken.xsl") {
            return Err(TransformError::new(Some(xsl), "壊れています"));
        }
        let name = xml.file_name().unwrap().to_string_lossy();
        Ok(format!("<html><head><title>{name}</title></head><body>{name}</body></html>"))
    }
}

/// 呼ばれた OS の機能を記録する仮の実装。
#[derive(Default, Clone)]
struct FakePlatform {
    calls: Arc<Mutex<Vec<String>>>,
    picked: Vec<PathBuf>,
    integration: Arc<Mutex<Vec<IntegrationItem>>>,
}

impl Platform for FakePlatform {
    fn open_file(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        self.calls.lock().unwrap().push(format!("file:{name}"));
        Ok(())
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("url:{url}"));
        Ok(())
    }
    fn pick_files(&self) -> Result<Vec<PathBuf>, String> {
        Ok(self.picked.clone())
    }
    fn integration_status(&self) -> Result<Vec<IntegrationItem>, String> {
        Ok(self.integration.lock().unwrap().clone())
    }
    fn integration_set(&self, key: &str, enabled: bool) -> Result<(), String> {
        let mut items = self.integration.lock().unwrap();
        let item = items.iter_mut().find(|i| i.key == key).ok_or("unknown key")?;
        item.state = if enabled { IntegrationState::On } else { IntegrationState::Off };
        Ok(())
    }
}

fn api(work: &Work, platform: FakePlatform, startup: Vec<PathBuf>) -> Api {
    Api::new(
        work.0.join("tmp"),
        RecentStore::new(work.0.join("recent.json")),
        Box::new(FakeTransformer),
        Box::new(platform),
        startup,
    )
}

fn call(api: &Api, cmd: &str, args: Value) -> Value {
    api.handle(cmd, args.to_string().as_bytes()).unwrap()
}

#[test]
fn startup_returns_paths_only_once() {
    let work = Work::new("startup");
    let api = api(&work, FakePlatform::default(), vec![PathBuf::from("C:\\a.zip")]);
    assert_eq!(call(&api, "startup", json!({})), json!({ "paths": ["C:\\a.zip"] }));
    assert_eq!(call(&api, "startup", json!({})), json!({ "paths": [] }));
}

#[test]
fn open_zip_returns_pages_and_others_and_records_history() {
    let work = Work::new("open");
    let zip = work.0.join("202601010000000001.zip");
    zip_fixture("egov", "202601010000000001", &zip);
    let api = api(&work, FakePlatform::default(), vec![]);

    let res = call(&api, "open", json!({ "paths": [zip.to_string_lossy()] }));
    let r = &res["results"][0];
    assert_eq!(r["ok"], true);
    assert_eq!(r["existing"], false);
    let source = &r["source"];
    assert_eq!(source["name"], "202601010000000001.zip");
    assert_eq!(source["kind"], "zip");
    assert_eq!(source["initialPage"], 0);
    assert_eq!(source["listOthers"], true);
    let pages = source["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[2]["file"], "henrei.xml");
    assert_eq!(pages[2]["folder"], "202601010000000001");
    assert_eq!(pages[2]["title"], "henrei.xml");
    assert_eq!(source["others"][0]["file"], "tsuchisho.pdf");

    let recent = call(&api, "recent", json!({}));
    assert_eq!(recent["items"][0]["path"], zip.to_string_lossy().as_ref());

    // 同じパスをもう一度開くと、既存のものを返す。
    let again = call(&api, "open", json!({ "paths": [zip.to_string_lossy()] }));
    assert_eq!(again["results"][0]["existing"], true);
    assert_eq!(again["results"][0]["source"]["id"], source["id"]);
}

#[test]
fn opening_page_of_open_source_selects_it() {
    let work = Work::new("open-merge");
    let api = api(&work, FakePlatform::default(), vec![]);
    let egov = fixture("egov");
    let path = |name: &str| egov.join(name).to_string_lossy().into_owned();

    // 同じフォルダの XML を 2 つ渡すと、まとまりは 1 つで、後の XML のページを選ぶ。
    let res = call(&api, "open", json!({ "paths": [path("202601010000000001.xml"), path("henrei.xml")] }));
    let (first, second) = (&res["results"][0], &res["results"][1]);
    assert_eq!(first["existing"], false);
    assert_eq!(second["existing"], true);
    assert_eq!(second["source"]["id"], first["source"]["id"]);
    let pages = second["source"]["pages"].as_array().unwrap();
    let henrei = pages.iter().position(|p| p["file"] == "henrei.xml").unwrap();
    assert_eq!(second["page"], henrei);
    // 最初の XML をもう一度開くと、そのページを指す。
    let again = call(&api, "open", json!({ "paths": [path("202601010000000001.xml")] }));
    assert_eq!(again["results"][0]["page"], first["source"]["initialPage"]);

    // フォルダを開いた後に、その中の XML を開いても、まとまりは増えない。
    let api = Api::new(
        work.0.join("tmp2"),
        RecentStore::new(work.0.join("recent2.json")),
        Box::new(FakeTransformer),
        Box::new(FakePlatform::default()),
        vec![],
    );
    let folder = call(&api, "open", json!({ "paths": [egov.to_string_lossy()] }));
    let xml = call(&api, "open", json!({ "paths": [path("henrei.xml")] }));
    assert_eq!(xml["results"][0]["existing"], true);
    assert_eq!(xml["results"][0]["source"]["id"], folder["results"][0]["source"]["id"]);
    assert_eq!(xml["results"][0]["page"], henrei);
    // フォルダそのものをもう一度開いたときは、ページを指さない（画面側で最後に見ていたページに戻す）。
    let again = call(&api, "open", json!({ "paths": [egov.to_string_lossy()] }));
    assert_eq!(again["results"][0]["existing"], true);
    assert_eq!(again["results"][0]["page"], Value::Null);
}

/// 変換に時間がかかる仮の実装。同じパスを並行して開いたときの確認に使う。
struct SlowTransformer;

impl Transformer for SlowTransformer {
    fn transform(&self, xml: &Path, xsl: &Path) -> Result<String, TransformError> {
        std::thread::sleep(std::time::Duration::from_millis(200));
        FakeTransformer.transform(xml, xsl)
    }
}

#[test]
fn opening_same_path_concurrently_keeps_one_source() {
    let work = Work::new("open-concurrent");
    let zip = work.0.join("202601010000000001.zip");
    zip_fixture("egov", "202601010000000001", &zip);
    let api = Arc::new(Api::new(
        work.0.join("tmp"),
        RecentStore::new(work.0.join("recent.json")),
        Box::new(SlowTransformer),
        Box::new(FakePlatform::default()),
        vec![],
    ));

    let args = json!({ "paths": [zip.to_string_lossy()] });
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (api, args) = (api.clone(), args.clone());
            std::thread::spawn(move || call(&api, "open", args))
        })
        .collect();
    let results: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()["results"][0].clone()).collect();

    // どちらも開けたことになり、同じものを指す。新しく開いたのは 1 つだけ。
    assert!(results.iter().all(|r| r["ok"] == true));
    assert_eq!(results[0]["source"]["id"], results[1]["source"]["id"]);
    assert_eq!(results.iter().filter(|r| r["existing"] == true).count(), 1);
    // 後から開き終わった方の展開先は、破棄されて残らない。
    let dirs_in = |dir: PathBuf| fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir());
    let extracted: usize = dirs_in(work.0.join("tmp")).map(|session| dirs_in(session).count()).sum();
    assert_eq!(extracted, 1);
}

#[test]
fn open_reports_failures_and_continues() {
    let work = Work::new("fail");
    let zip = work.0.join("ok.zip");
    zip_fixture("egov", "x", &zip);
    let api = api(&work, FakePlatform::default(), vec![]);
    let res =
        call(&api, "open", json!({ "paths": [work.0.join("missing.zip").to_string_lossy(), zip.to_string_lossy()] }));
    assert_eq!(res["results"][0]["ok"], false);
    assert_eq!(res["results"][0]["error"]["kind"], "not_found");
    assert_eq!(res["results"][1]["ok"], true);
}

#[test]
fn page_returns_html_or_error() {
    let work = Work::new("page");
    let dir = work.0.join("broken");
    fs::create_dir_all(&dir).unwrap();
    for f in ["broken.xml", "broken.xsl", "ok.xml", "ok.xsl"] {
        fs::copy(fixture("broken").join(f), dir.join(f)).unwrap();
    }
    let api = api(&work, FakePlatform::default(), vec![]);
    let res = call(&api, "open", json!({ "paths": [dir.to_string_lossy()] }));
    let id = res["results"][0]["source"]["id"].as_u64().unwrap();
    assert_eq!(res["results"][0]["source"]["pages"][0]["failed"], true);

    let failed = call(&api, "page", json!({ "source": id, "page": 0 }));
    assert_eq!(failed["html"], Value::Null);
    assert_eq!(failed["error"]["file"], "broken.xsl");
    let ok = call(&api, "page", json!({ "source": id, "page": 1 }));
    assert!(ok["html"].as_str().unwrap().contains("ok.xml"));
    assert_eq!(ok["error"], Value::Null);

    let missing = api.handle("page", json!({ "source": id, "page": 9 }).to_string().as_bytes());
    assert_eq!(missing.unwrap_err().kind, "bad_request");
}

#[test]
fn links_switch_pages_or_call_platform() {
    let work = Work::new("link");
    let zip = work.0.join("doc.zip");
    zip_fixture("egov", "doc", &zip);
    let platform = FakePlatform::default();
    let api = api(&work, platform.clone(), vec![]);
    let res = call(&api, "open", json!({ "paths": [zip.to_string_lossy()] }));
    let id = res["results"][0]["source"]["id"].as_u64().unwrap();

    let link = |href: &str| call(&api, "link", json!({ "source": id, "page": 0, "href": href }));
    assert_eq!(link("henrei.xml"), json!({ "kind": "page", "page": 2 }));
    assert_eq!(link("tsuchisho.pdf"), json!({ "kind": "opened" }));
    assert_eq!(link("https://www.example.jp/"), json!({ "kind": "opened" }));
    assert_eq!(link("javascript:alert(1)"), json!({ "kind": "none" }));
    assert_eq!(link("C:\\Windows\\notepad.exe"), json!({ "kind": "none" }));

    call(&api, "openOther", json!({ "source": id, "index": 0 }));
    assert_eq!(
        *platform.calls.lock().unwrap(),
        ["file:tsuchisho.pdf", "url:https://www.example.jp/", "file:tsuchisho.pdf"]
    );
}

#[test]
fn close_removes_source_and_extracted_files() {
    let work = Work::new("close");
    let zip = work.0.join("doc.zip");
    zip_fixture("egov", "doc", &zip);
    let api = api(&work, FakePlatform::default(), vec![]);
    let res = call(&api, "open", json!({ "paths": [zip.to_string_lossy()] }));
    let id = res["results"][0]["source"]["id"].as_u64().unwrap();
    let session_dir = fs::read_dir(work.0.join("tmp")).unwrap().next().unwrap().unwrap().path();
    assert!(session_dir.join("1").exists());

    call(&api, "close", json!({ "source": id }));
    assert!(!session_dir.join("1").exists());
    let err = api.handle("page", json!({ "source": id, "page": 0 }).to_string().as_bytes()).unwrap_err();
    assert_eq!(err.kind, "not_open");

    api.shutdown();
    assert!(!session_dir.exists());
}

#[test]
fn pick_and_recent_commands() {
    let work = Work::new("recent");
    let platform = FakePlatform { picked: vec![PathBuf::from("C:\\x.zip")], ..FakePlatform::default() };
    let api = api(&work, platform, vec![]);
    assert_eq!(call(&api, "pick", json!({})), json!({ "paths": ["C:\\x.zip"] }));

    let dir = work.0.join("d");
    fs::create_dir_all(&dir).unwrap();
    call(&api, "open", json!({ "paths": [dir.to_string_lossy()] }));
    assert_eq!(call(&api, "recent", json!({}))["items"].as_array().unwrap().len(), 1);
    let removed = call(&api, "recentRemove", json!({ "path": dir.to_string_lossy() }));
    assert_eq!(removed["items"], json!([]));
    call(&api, "open", json!({ "paths": [dir.to_string_lossy()] }));
    assert_eq!(call(&api, "recentClear", json!({}))["items"], json!([]));
    assert_eq!(call(&api, "recent", json!({}))["items"], json!([]));
}

#[test]
fn integration_set_and_repair() {
    let work = Work::new("integration");
    let platform = FakePlatform::default();
    *platform.integration.lock().unwrap() = vec![
        IntegrationItem { key: "sendTo".into(), state: IntegrationState::Stale },
        IntegrationItem { key: "contextMenu".into(), state: IntegrationState::Off },
    ];
    let api = api(&work, platform, vec![]);
    let res = call(&api, "integrationSet", json!({ "key": "contextMenu", "enabled": true }));
    assert_eq!(res["items"][1]["state"], "on");
    let res = call(&api, "integrationRepair", json!({}));
    assert_eq!(res["items"][0]["state"], "on");
}

#[test]
fn rejects_unknown_commands_and_bad_arguments() {
    let work = Work::new("bad");
    let api = api(&work, FakePlatform::default(), vec![]);
    assert_eq!(api.handle("rm", b"{}").unwrap_err().kind, "unknown_command");
    assert_eq!(api.handle("open", b"{\"paths\": 1}").unwrap_err().kind, "bad_request");
    assert_eq!(api.handle("page", b"not json").unwrap_err().kind, "bad_request");
}

#[test]
fn too_large_zip_is_reported() {
    let work = Work::new("large");
    let zip = work.0.join("doc.zip");
    zip_fixture("egov", "doc", &zip);
    let api = api(&work, FakePlatform::default(), vec![]).with_limits(Limits { max_entries: 2, ..Limits::default() });
    let res = call(&api, "open", json!({ "paths": [zip.to_string_lossy()] }));
    assert_eq!(res["results"][0]["error"]["kind"], "too_large");
}
