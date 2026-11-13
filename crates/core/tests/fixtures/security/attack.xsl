<?xml version="1.0" encoding="UTF-8"?>
<!-- テスト用: 表示側の防御（スクリプト停止・外部通信の遮断・遷移の阻止）を確かめるための文書。 -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="html"/>
<xsl:template match="/">
<html><head><title>防御の確認用</title>
<meta http-equiv="refresh" content="1;url=https://www.example.com/refresh"/>
<script>document.title = 'SCRIPT RAN'; window.parent.postMessage('script ran', '*');</script>
<link rel="stylesheet" href="https://www.example.com/external.css"/>
</head><body onload="document.title='ONLOAD RAN'">
<p id="msg">このページのスクリプトが動いたら、タイトルが書き換わる。</p>
<img id="ext" src="https://www.example.com/beacon.png" alt="外部画像"/>
<img id="local" src="local.png" alt="相対パスの画像"/>
<form id="f" action="https://www.example.com/post" method="post"><input name="x" value="1"/><button id="submit" type="submit">送信</button></form>
<a id="top" href="https://www.example.com/top" target="_top">上位に遷移するリンク</a>
<a id="js" href="javascript:document.title='JS LINK RAN'">javascript リンク</a>
<iframe id="inner" src="https://www.example.com/frame"></iframe>
</body></html>
</xsl:template>
</xsl:stylesheet>
