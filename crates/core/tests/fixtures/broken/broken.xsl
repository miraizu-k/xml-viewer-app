<?xml version="1.0" encoding="UTF-8"?>
<!-- テスト用: 終了タグが一致しない、壊れた XSL。 -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:template match="/">
	<html><body>
		<xsl:choose>
			<xsl:when test="doc">x</xsl:when>
		</xsl:if>
	</body></html>
</xsl:template>
</xsl:stylesheet>
