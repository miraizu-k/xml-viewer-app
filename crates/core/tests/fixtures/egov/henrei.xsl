<?xml version="1.0" encoding="UTF-8"?>
<!-- テスト用の返戻票の XSL。xsl:output が無く、ルートが html 要素。disable-output-escaping で値の中のタグを出す。 -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
	<xsl:template match="/">
		<html>
			<head>
				<style type="text/css">
					table.outline { border: 1px solid #000000; width: 640px; }
					.title { font-size: 14pt; font-weight: bold; text-align: center; }
				</style>
				<title>届書（申請書等）の補正に関するお願い</title>
			</head>
			<body>
				<table class="outline"><tr><td><xsl:apply-templates/></td></tr></table>
			</body>
		</html>
	</xsl:template>

	<xsl:template match="_返戻票">
		<p class="title">届書（申請書等）の補正に関するお願い</p>
		<p><xsl:value-of select="_申請者法人団体名"/><br/><xsl:value-of select="_申請者"/>　様</p>
		<p id="boutou"><xsl:value-of select="_冒頭文書" disable-output-escaping="yes"/></p>
		<p>＜返戻理由＞</p>
		<xsl:apply-templates select="_返戻理由"/>
	</xsl:template>

	<xsl:template match="_返戻理由">
		<xsl:if test="substring(.,1,6)='&lt;DIV/>'">
			<p id="riyuu">・<xsl:value-of select="." disable-output-escaping="yes"/></p>
		</xsl:if>
	</xsl:template>
</xsl:stylesheet>
