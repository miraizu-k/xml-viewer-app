# XML Viewer

e-Gov などで受け取る電子公文書（XML + XSL）を、Windows でそのまま見るためのビューアです。
zip をウィンドウにドロップすると、中の表示できるページが一覧に並び、選んだページを表示します。

A viewer for Japanese electronic official documents (XML + XSL, e.g. from e-Gov) on Windows.
Drop a zip and it lists the pages that have an XSL stylesheet, then shows the selected page transformed by the MSXML 6 built into Windows.

## ダウンロード

[Releases](../../releases) から `xml-viewer.exe` をダウンロードします。インストールは要りません。

- Windows 10 / 11（64 ビット）
- Microsoft Edge WebView2 ランタイム（Windows 11 と、更新済みの Windows 10 には入っています）

Visual C++ ランタイムは exe に含めているので、別に入れる必要はありません。
exe に署名をしていないので、初めて起動するときに SmartScreen の警告が出ることがあります。
「詳細情報」→「実行」で起動できます。

## 使い方

- 開き方: exe のアイコンにドロップ、ウィンドウにドロップ、「開く」ボタン、コマンドライン（`xml-viewer.exe パス...`）
- 開けるもの: zip、XML（`xml-stylesheet` で指定された同じフォルダの XSL を読む）、フォルダ
- 文書の中のリンクで、ほかのページに移れます。PDF などアプリで表示しないファイルは、既定のアプリで開きます
- 印刷は Ctrl + P。PDF は印刷ダイアログの「PDF として保存」で作ります
- 何も開いていないときの画面に、最近開いたファイルが出ます
- 「設定」から、エクスプローラーの「送る」・右クリックメニュー・「プログラムから開く」に登録できます。今のユーザーだけの設定で、管理者権限は要りません

## できないこと

- Windows 以外での動作
- 電子署名の検証、XML の編集
- `xsl:import` / `xsl:include` を使う XSL と、DOCTYPE 宣言を持つ XML の表示（MSXML を安全な既定の設定のまま使っているため）

## プライバシーと安全性

- このアプリは、利用者が求めた場合を除き、ほかのネットワーク上のシステムに情報を送りません。文書の中の `http://` / `https://` のリンクは、クリックしたときだけ既定のブラウザで開きます
- 文書の中のスクリプトは実行しません。文書から外部への通信（画像や CSS の読み込みなど）も止めます
- XSL の変換には Windows の MSXML 6 を、スクリプトや外部ファイルを使えない既定の設定で使います
- zip は一時フォルダ（`%TEMP%\xml-viewer`）に展開し、閉じたときとアプリの終了時に削除します。展開先の外を指すエントリは展開しません
- 最近開いたファイルとして記録するのはパスと日時だけで、文書の中身は保存しません

## アンインストール

インストーラーはありません。次の順に消します。

1. 「設定」で「送る」「右クリックメニュー」「プログラムから開く」をすべて切る（登録したものが消えます）
2. exe を削除する
3. 必要なら、アプリが作ったフォルダを削除する
   - `%APPDATA%\xml-viewer`（最近開いたファイルの記録）
   - `%LOCALAPPDATA%\xml-viewer`（WebView2 のデータ）
   - `%TEMP%\xml-viewer`（展開した zip。通常は終了時に消えます）

先に exe を消した場合、登録は次の場所に残ります（今のユーザーの分だけ）。手で削除できます。
`xml-viewer.exe` の部分は、exe の名前を変えていればその名前です。

- `HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.zip\shell\XmlViewer.Open`（`.xml` も同じ）
- `HKEY_CURRENT_USER\Software\Classes\Applications\xml-viewer.exe`
- `HKEY_CURRENT_USER\Software\Classes\.zip\OpenWithList\xml-viewer.exe`（`.xml` も同じ）
- 「送る」フォルダ（エクスプローラーのアドレス欄に `shell:sendto`）の `XML Viewer.lnk`

## 不具合の報告

[Issues](../../issues) で受け付けています。

**実物の公文書（氏名・住所・番号などを含むもの）は、添付も貼り付けもしないでください。**
Issue は誰でも見られます。再現に文書が必要なときは、個人情報を架空のものに置き換えたデータを使ってください。

脆弱性は公開の Issue ではなく、[SECURITY.md](SECURITY.md) の方法で知らせてください。

## 開発

- `crates/core`: 開く・zip の展開・一覧・変換・リンク・履歴。MSXML を呼ぶ部分（`xslt.rs`）だけが Windows 専用で、それ以外は Linux でもテストできます
- `crates/app`: exe。`src/api.rs` が画面からの呼び出しを振り分け、`src/win/` がウィンドウ（wry + tao / WebView2）と Windows の機能を受け持ちます。画面は `ui/` の HTML / CSS / JS で、exe に埋め込みます
- テスト用のデータ（`crates/core/tests/fixtures/`）は、すべて架空のダミーです

```sh
# Windows 上
cargo test --workspace
cargo build --release -p xml-viewer

# WSL / Linux から（cargo-xwin を使う）
cargo test --workspace   # Windows に依存しない部分だけ
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=env cargo xwin test --target x86_64-pc-windows-msvc --workspace   # WSL から exe を直接起動する
cargo xwin build --release --target x86_64-pc-windows-msvc -p xml-viewer
```

`main` への変更は PR で行い、Linux と Windows の CI が通ってからマージします。

リリースは、`Cargo.toml` の `version` を上げる PR をマージしてから、`main` のそのコミットに同じ番号のタグ（例: `v0.2.0`）を付けて push します。
GitHub Actions が Windows でテストとビルドをして、Releases に exe を載せます。`v*` のタグは、一度 push すると消したり付け替えたりできません。

## ライセンス

[MIT](LICENSE)。exe に含まれるサードパーティのソフトウェアのライセンスは、各リリースの `THIRD-PARTY-LICENSES.txt` にまとめています
（`cargo about generate --workspace --locked --fail about.hbs -o THIRD-PARTY-LICENSES.txt` で手元でも作れます）。
