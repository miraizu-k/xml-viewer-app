//! Windows 向けビルドで、exe にアプリケーション マニフェストを埋め込む。
//!
//! embed-manifest は MSVC ターゲットではリンカの `/MANIFEST:EMBED` を使うが、
//! Linux からのクロスビルドで使う rust-lld はこのとき `mt.exe` を要求する。
//! そこでクロスビルドのときだけ、GNU ターゲット向けの経路（マニフェストを `.rsrc` セクションの
//! COFF オブジェクトにしてリンカへ直接渡す）を使う。外部ツールが要らず、lld-link でも取り込める。
//! Windows 上でのビルド（CI など）は、MSVC の標準の経路のまま。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }
    let host = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();
    if !host.contains("-windows-")
        && let Some(arch) = target.strip_suffix("-pc-windows-msvc")
    {
        // SAFETY: ビルドスクリプトは単一スレッドで動いており、ほかに環境変数を読むスレッドは無い。
        unsafe { std::env::set_var("TARGET", format!("{arch}-pc-windows-gnu")) };
    }
    embed_manifest::embed_manifest(embed_manifest::new_manifest("XmlViewer"))
        .expect("マニフェストを埋め込めませんでした");
}
