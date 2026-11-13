//! MSXML 6 による変換の結合テスト（Windows 専用）。
//!
//! 実物の公文書での確認は、環境変数 `XMLV_REAL_SAMPLES` に zip のあるフォルダを渡したときだけ行う。
//! 実物はリポジトリに入れない。
#![cfg(windows)]

mod common;

use std::fs;
use std::path::Path;

use common::{Work, copy_dir, fixture};
use xml_viewer_core::source::{Limits, open};
use xml_viewer_core::temp::TempSession;
use xml_viewer_core::xslt::{Msxml, Transformer};

fn transform(dir: &Path, xml: &str, xsl: &str) -> String {
    Msxml.transform(&dir.join(xml), &dir.join(xsl)).unwrap()
}

#[test]
fn transforms_kagami_with_format_number_and_links() {
    let work = Work::new("msxml-kagami");
    copy_dir(&fixture("egov"), &work.0);
    let html = transform(&work.0, "202601010000000001.xml", "kagami.xsl");
    assert!(html.contains("<TITLE>架空機構からのお知らせ</TITLE>"), "{html}");
    assert!(html.contains("令和8年1月5日"), "{html}");
    assert!(html.contains("href=\"henrei.xml\""), "{html}");
    assert!(html.contains("年金　太郎　様"), "{html}");
}

#[test]
fn transforms_henrei_with_disable_output_escaping() {
    let work = Work::new("msxml-henrei");
    copy_dir(&fixture("egov"), &work.0);
    let html = transform(&work.0, "henrei.xml", "henrei.xsl");
    assert!(html.contains("再提出してください。<BR/>本内容"), "{html}");
    assert!(!html.contains("&lt;BR"), "{html}");
    assert!(html.contains("<title>届書（申請書等）の補正に関するお願い</title>"), "{html}");
}

#[test]
fn transforms_form_with_text_predicate_and_data_uri() {
    let work = Work::new("msxml-form");
    copy_dir(&fixture("egov"), &work.0);
    let html = transform(&work.0, "AD0000000000000001.xml", "AS0000000000000.xsl");
    assert!(html.starts_with("<!DOCTYPE html SYSTEM \"about:legacy-compat\">"), "{}", &html[..80]);
    assert!(html.contains("data:image/png;base64,"));
    assert!(html.contains("サンプル商事\u{a0}株式会社"), "{html}");
    assert!(html.contains("一行目<BR>二行目"), "{html}");
    assert!(html.contains("▲1,234"), "{html}");
}

#[test]
fn reports_broken_stylesheet_with_position() {
    let work = Work::new("msxml-broken");
    copy_dir(&fixture("broken"), &work.0);
    let err = Msxml.transform(&work.0.join("broken.xml"), &work.0.join("broken.xsl")).unwrap_err();
    assert_eq!(err.file.as_deref(), Some("broken.xsl"));
    assert!(err.line.is_some(), "{err:?}");
    assert!(!err.reason.is_empty());
}

#[test]
fn reports_missing_file() {
    let work = Work::new("msxml-missing");
    let err = Msxml.transform(&work.0.join("none.xml"), &work.0.join("none.xsl")).unwrap_err();
    assert_eq!(err.file.as_deref(), Some("none.xml"));
}

#[test]
fn transforms_paths_with_japanese_spaces_and_hash() {
    let work = Work::new("msxml-names");
    let dir = work.path("公文書 #1");
    copy_dir(&fixture("egov"), &dir);
    let html = transform(&dir, "henrei.xml", "henrei.xsl");
    assert!(html.contains("補正に関するお願い"));
}

#[test]
fn opens_real_samples_when_given() {
    let Some(dir) = std::env::var_os("XMLV_REAL_SAMPLES") else {
        eprintln!("XMLV_REAL_SAMPLES が無いので、実物での確認を飛ばします");
        return;
    };
    let work = Work::new("msxml-real");
    let session = TempSession::create(&work.path("tmp")).unwrap();
    let mut count = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
            continue;
        }
        let source = open(&path, &|| session.new_extract_dir(), &Msxml, &Limits::default()).unwrap();
        assert!(!source.pages.is_empty(), "{}", path.display());
        for page in &source.pages {
            assert!(page.html.is_ok(), "{}: {:?}", page.rel.join("/"), page.html.as_ref().err());
            count += 1;
        }
    }
    assert!(count > 0, "zip が見つかりませんでした");
    eprintln!("実物 {count} ページを変換しました");
}
