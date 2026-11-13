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
- 文書だけを拡大・縮小できます（ツールバーの − / +、Ctrl + ホイール、Ctrl + + / Ctrl + −、Ctrl + 0 で 100%）。印刷は 100% のままです
- 印刷は Ctrl + P。PDF は印刷ダイアログの「PDF として保存」で作ります
- 何も開いていないときの画面に、最近開いたファイルが出ます
- 「設定」から、エクスプローラーの「送る」・右クリックメニュー・「プログラムから開く」に登録できます。今のユーザーだけの設定で、管理者権限は要りません。切ると登録を消します

## プライバシーと安全性

- このアプリは、利用者が求めた場合を除き、ほかのネットワーク上のシステムに情報を送りません。文書の中の `http://` / `https://` のリンクは、クリックしたときだけ既定のブラウザで開きます
- 文書の中のスクリプトは実行しません。文書から外部への通信（画像や CSS の読み込みなど）も止めます
- XSL の変換には Windows の MSXML 6 を、スクリプトや外部ファイルを使えない既定の設定で使います
- zip は一時フォルダ（`%TEMP%\xml-viewer`）に展開し、閉じたときとアプリの終了時に削除します。展開先の外を指すエントリは展開しません
- 最近開いたファイルとして記録するのはパスと日時だけで、文書の中身は保存しません

## 不具合の報告

[Issues](../../issues) で受け付けています。

**実物の公文書（氏名・住所・番号などを含むもの）は、添付も貼り付けもしないでください。**
Issue は誰でも見られます。再現に文書が必要なときは、個人情報を架空のものに置き換えたデータを使ってください。

脆弱性は公開の Issue ではなく、[SECURITY.md](SECURITY.md) の方法で知らせてください。

## ライセンス

[MIT](LICENSE)。exe に含まれるサードパーティのソフトウェアのライセンスは、各リリースの `THIRD-PARTY-LICENSES.txt` にまとめています。
