//! 開く・分類・リンク解決の結合テスト（OS に依存しない部分。変換は仮のものを使う）。

mod common;

use std::fs;
use std::path::Path;

use common::{FakeTransformer, Work, copy_dir, fixture, raw_zip, zip_dir};
use xml_viewer_core::link::{LinkTarget, resolve_link};
use xml_viewer_core::source::{InputKind, Limits, OpenError, Source, open};
use xml_viewer_core::temp::TempSession;

fn open_with(path: &Path, work: &Work, limits: &Limits) -> Result<Source, OpenError> {
    let session = TempSession::create(&work.path("tmp")).unwrap();
    // Source が展開先を持つので、セッションは閉じずに残しておく（テストの終わりに Work ごと消える）。
    let session = Box::leak(Box::new(session));
    open(path, &|| session.new_extract_dir(), &FakeTransformer, limits)
}

fn page_files(source: &Source) -> Vec<String> {
    source.pages.iter().map(|p| p.rel.join("/")).collect()
}

fn other_files(source: &Source) -> Vec<String> {
    source.others.iter().map(|o| o.rel.join("/")).collect()
}

#[test]
fn opens_zip_with_three_pages() {
    let work = Work::new("zip3");
    let zip = work.path("202601010000000001.zip");
    zip_dir(&fixture("egov"), "202601010000000001", &zip);

    let mut source = open_with(&zip, &work, &Limits::default()).unwrap();
    assert_eq!(source.kind, InputKind::Zip);
    assert_eq!(source.name(), "202601010000000001.zip");
    assert_eq!(
        page_files(&source),
        [
            "202601010000000001/202601010000000001.xml",
            "202601010000000001/AD0000000000000001.xml",
            "202601010000000001/henrei.xml",
        ]
    );
    assert_eq!(source.pages[0].title, "202601010000000001.xml by kagami.xsl");
    assert_eq!(other_files(&source), ["202601010000000001/tsuchisho.pdf"]);
    assert!(source.list_others);
    assert_eq!(source.initial_page, Some(0));
    assert!(!source.truncated);

    // 閉じると展開先が消える。
    let root = source.root.clone();
    assert!(root.exists());
    source.close();
    assert!(!root.exists());
}

#[test]
fn opens_folder_recursively() {
    let work = Work::new("folder");
    copy_dir(&fixture("egov"), &work.path("docs/sub"));
    let source = open_with(&work.path("docs"), &work, &Limits::default()).unwrap();
    assert_eq!(source.kind, InputKind::Folder);
    assert_eq!(page_files(&source).len(), 3);
    assert!(page_files(&source)[0].starts_with("sub/"));
}

#[test]
fn opening_xml_selects_it_and_hides_others() {
    let work = Work::new("xml");
    copy_dir(&fixture("egov"), &work.path("docs"));
    let henrei = work.path("docs/henrei.xml");
    let mut source = open_with(&henrei, &work, &Limits::default()).unwrap();
    assert_eq!(source.kind, InputKind::Xml);
    assert_eq!(source.root, work.path("docs"));
    assert_eq!(source.initial_page, Some(2));
    assert_eq!(source.pages[2].rel, ["henrei.xml"]);
    assert!(!source.list_others);
    // 直接開いたフォルダは、閉じても消さない。
    source.close();
    assert!(henrei.exists());
}

#[test]
fn opening_xml_without_stylesheet_has_no_initial_page() {
    let work = Work::new("plain");
    copy_dir(&fixture("missing"), &work.path("docs"));
    let source = open_with(&work.path("docs/plain.xml"), &work, &Limits::default()).unwrap();
    assert!(source.pages.is_empty());
    assert_eq!(source.initial_page, None);
}

#[test]
fn xml_without_usable_stylesheet_is_other_file() {
    let work = Work::new("missing");
    copy_dir(&fixture("missing"), &work.path("docs"));
    let source = open_with(&work.path("docs"), &work, &Limits::default()).unwrap();
    assert!(source.pages.is_empty());
    assert_eq!(source.initial_page, None);
    assert_eq!(other_files(&source), ["orphan.xml", "plain.xml", "remote.xml"]);
}

#[test]
fn pdf_only_zip_has_no_pages() {
    let work = Work::new("pdf");
    let zip = work.path("pdf.zip");
    fs::write(&zip, raw_zip(&[(b"tsuchi/a.pdf", b"%PDF-1.4"), (b"tsuchi/b.pdf", b"%PDF-1.4")])).unwrap();
    let source = open_with(&zip, &work, &Limits::default()).unwrap();
    assert!(source.pages.is_empty());
    assert_eq!(other_files(&source), ["tsuchi/a.pdf", "tsuchi/b.pdf"]);
}

#[test]
fn broken_stylesheet_fails_only_that_page() {
    let work = Work::new("broken");
    copy_dir(&fixture("broken"), &work.path("docs"));
    let source = open_with(&work.path("docs"), &work, &Limits::default()).unwrap();
    assert_eq!(page_files(&source), ["broken.xml", "ok.xml"]);
    let err = source.pages[0].html.as_ref().unwrap_err();
    assert_eq!(err.file.as_deref(), Some("broken.xsl"));
    assert_eq!(err.line, Some(8));
    assert_eq!(source.pages[0].title, "broken.xml");
    assert!(source.pages[1].html.is_ok());
}

#[test]
fn unsafe_zip_entries_are_not_extracted_outside() {
    let work = Work::new("slip");
    let zip = work.path("evil.zip");
    fs::write(
        &zip,
        raw_zip(&[
            (b"../evil.txt", b"x"),
            (b"a/../../evil2.txt", b"x"),
            (b"/abs.txt", b"x"),
            (b"C:/drive.txt", b"x"),
            (b"ok/fine.txt", b"x"),
        ]),
    )
    .unwrap();
    let source = open_with(&zip, &work, &Limits::default()).unwrap();
    assert_eq!(source.skipped_entries, 4);
    assert_eq!(other_files(&source), ["ok/fine.txt"]);
    let parent = source.root.parent().unwrap();
    assert!(!parent.join("evil.txt").exists());
    assert!(!work.path("evil.txt").exists());
    assert!(!work.path("evil2.txt").exists());
}

#[test]
fn shift_jis_names_are_decoded() {
    let work = Work::new("sjis");
    let (name, _, _) = encoding_rs::SHIFT_JIS.encode("公文書/返戻票.pdf");
    let zip = work.path("sjis.zip");
    fs::write(&zip, raw_zip(&[(&name, b"%PDF")])).unwrap();
    let source = open_with(&zip, &work, &Limits::default()).unwrap();
    assert_eq!(other_files(&source), ["公文書/返戻票.pdf"]);
}

#[test]
fn rejects_too_many_entries_and_too_large_total() {
    let work = Work::new("limits");
    let zip = work.path("big.zip");
    fs::write(&zip, raw_zip(&[(b"a.txt", &[b'a'; 600]), (b"b.txt", &[b'b'; 600])])).unwrap();

    let few = Limits { max_entries: 1, ..Limits::default() };
    assert!(matches!(open_with(&zip, &work, &few), Err(OpenError::TooLarge(_))));

    let small = Limits { max_total_bytes: 1000, ..Limits::default() };
    assert!(matches!(open_with(&zip, &work, &small), Err(OpenError::TooLarge(_))));
    // 失敗したら作りかけの展開先は消える。
    let leftovers: Vec<_> = fs::read_dir(work.path("tmp"))
        .unwrap()
        .flat_map(|session| fs::read_dir(session.unwrap().path()).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".lock")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn reports_missing_unsupported_and_corrupt_inputs() {
    let work = Work::new("errors");
    assert!(matches!(open_with(&work.path("nothing.zip"), &work, &Limits::default()), Err(OpenError::NotFound)));

    fs::write(work.path("a.txt"), "hello").unwrap();
    assert!(matches!(open_with(&work.path("a.txt"), &work, &Limits::default()), Err(OpenError::Unsupported)));

    fs::write(work.path("bad.zip"), "PK\x03\x04 not really a zip").unwrap();
    assert!(matches!(open_with(&work.path("bad.zip"), &work, &Limits::default()), Err(OpenError::InvalidZip(_))));
}

#[test]
fn truncates_pages_over_limit() {
    let work = Work::new("many");
    let dir = work.path("docs");
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture("broken/ok.xsl"), dir.join("ok.xsl")).unwrap();
    for i in 0..5 {
        fs::copy(fixture("broken/ok.xml"), dir.join(format!("p{i}.xml"))).unwrap();
    }
    let limits = Limits { max_pages: 3, ..Limits::default() };
    let source = open_with(&dir, &work, &limits).unwrap();
    assert_eq!(source.pages.len(), 3);
    assert!(source.truncated);

    // XML を直接開いたときは、上限で切られる側の XML でも必ず一覧に入る。
    let source = open_with(&dir.join("p4.xml"), &work, &limits).unwrap();
    assert_eq!(source.pages.len(), 3);
    assert_eq!(source.initial_page.map(|i| source.pages[i].rel.join("/")), Some("p4.xml".to_string()));
}

#[test]
fn resolves_links_between_pages_files_and_web() {
    let work = Work::new("links");
    let zip = work.path("doc.zip");
    zip_dir(&fixture("egov"), "202601010000000001", &zip);
    let source = open_with(&zip, &work, &Limits::default()).unwrap();
    let kagami = 0;
    assert_eq!(resolve_link(&source, kagami, "henrei.xml"), LinkTarget::Page(2));
    assert_eq!(resolve_link(&source, kagami, "./HENREI.XML"), LinkTarget::Page(2));
    assert_eq!(resolve_link(&source, kagami, "tsuchisho.pdf"), LinkTarget::Other(0));
    assert_eq!(
        resolve_link(&source, kagami, "https://www.example.jp/info.html"),
        LinkTarget::External("https://www.example.jp/info.html".to_string())
    );
    assert_eq!(resolve_link(&source, kagami, "javascript:alert(1)"), LinkTarget::None);
    assert_eq!(resolve_link(&source, kagami, "file:///C:/Windows/notepad.exe"), LinkTarget::None);
    assert_eq!(resolve_link(&source, kagami, "../../outside.xml"), LinkTarget::None);
    assert_eq!(resolve_link(&source, kagami, "kagami.xsl"), LinkTarget::None);
    assert_eq!(resolve_link(&source, kagami, "#top"), LinkTarget::None);
    assert_eq!(resolve_link(&source, 99, "henrei.xml"), LinkTarget::None);
}
