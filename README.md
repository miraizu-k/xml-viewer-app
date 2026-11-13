# XML Viewer

e-Gov などで受け取る電子公文書（XML + XSL）を、Windows でそのまま見るためのビューアです。
zip をウィンドウにドロップすると、中の表示できるページが一覧に並び、選んだページを表示します。

## ダウンロード

[Releases](../../releases) から `xml-viewer.exe` をダウンロードします。インストールは要りません。

- Windows 10 / 11（64 ビット）
- Microsoft Edge WebView2 ランタイム（Windows 11 と、更新済みの Windows 10 には入っています）

exe に署名をしていないので、初めて起動するときに SmartScreen の警告が出ることがあります。

## 使い方

- 開き方: exe のアイコンにドロップ、ウィンドウにドロップ、「開く」ボタン、コマンドライン（`xml-viewer.exe パス`）
- 開けるもの: zip、XML（同じフォルダの XSL を読む）、フォルダ
- 印刷は Ctrl + P。PDF は印刷ダイアログの「PDF として保存」で作ります
- 「設定」から、エクスプローラーの「送る」・右クリックメニュー・「プログラムから開く」に登録できます。今のユーザーだけの設定で、管理者権限は要りません

zip は一時フォルダに展開し、閉じたときとアプリの終了時に削除します。文書の中のスクリプトは動かさず、文書から外部への通信もさせません。

## ビルド

```sh
# Windows 上
cargo build --release -p xml-viewer

# WSL / Linux から（cargo-xwin を使う）
cargo xwin build --release --target x86_64-pc-windows-msvc -p xml-viewer
```

## リリース

`Cargo.toml` の `version` を上げてコミットし、同じ番号のタグ（例: `v0.2.0`）を push すると、
GitHub Actions が Windows で exe をビルドし、Releases に載せます。
