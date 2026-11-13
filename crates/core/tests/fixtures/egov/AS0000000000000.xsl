<?xml version="1.0" encoding="UTF-8"?>
<!-- テスト用の様式の XSL。DOCTYPE 付きの HTML 出力、decimal-format、text()[1]、translate、
     名前付きテンプレートの再帰、data URI の画像、絶対配置の要素を使う。 -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output encoding="utf-8" doctype-system="about:legacy-compat" method="html"/>
    <xsl:decimal-format NaN="" minus-sign="▲" name="tri-min" />
    <xsl:template match="/">
        <HTML lang="ja">
        <HEAD>
            <TITLE>テスト用の届書（処理票）</TITLE>
            <STYLE type="text/css">
              .field { position: absolute; box-sizing: border-box; background-color: #FCF3C3; font-size: 12px; }
            </STYLE>
        </HEAD>
        <BODY>
          <DIV style="position:relative;">
            <IMG id="tool_img" style="position:relative; width:400px; height:200px;" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEUlEQVR4nGN48eYDVsQwtCQAJtOxAYF7kisAAAAASUVORK5CYII="/>
            <xsl:element name="SPAN">
                <xsl:attribute name="id">nen</xsl:attribute>
                <xsl:attribute name="class">field</xsl:attribute>
                <xsl:attribute name="style">left: 20px; top: 20px; width: 40px; height: 20px;</xsl:attribute>
                <xsl:call-template name="ins-br">
                    <xsl:with-param name="value" select="translate(/DataRoot/G00003-A-000000-001_1/提出年月日/年/text()[1], ' ','&#160;')" />
                </xsl:call-template>
            </xsl:element>
            <xsl:element name="SPAN">
                <xsl:attribute name="id">meisho</xsl:attribute>
                <xsl:attribute name="class">field</xsl:attribute>
                <xsl:attribute name="style">left: 20px; top: 60px; width: 300px; height: 20px;</xsl:attribute>
                <xsl:call-template name="ins-br">
                    <xsl:with-param name="value" select="translate(/DataRoot/G00003-A-000000-001_1/事業所名称/text()[1], '　','&#160;')" />
                </xsl:call-template>
            </xsl:element>
            <xsl:element name="SPAN">
                <xsl:attribute name="id">biko</xsl:attribute>
                <xsl:attribute name="class">field</xsl:attribute>
                <xsl:attribute name="style">left: 20px; top: 100px; width: 300px; height: 40px;</xsl:attribute>
                <xsl:call-template name="ins-br">
                    <xsl:with-param name="value" select="/DataRoot/G00003-A-000000-001_1/備考/text()[1]" />
                </xsl:call-template>
            </xsl:element>
            <SPAN id="kingaku" class="field" style="left: 20px; top: 150px; width: 100px; height: 20px;">
                <xsl:value-of select="format-number(/DataRoot/G00003-A-000000-001_1/金額, '#,##0', 'tri-min')"/>
            </SPAN>
            <SPAN id="kuuran" class="field" style="left: 140px; top: 150px; width: 100px; height: 20px;"><xsl:value-of select="/DataRoot/G00003-A-000000-001_1/空欄"/></SPAN>
          </DIV>
        </BODY>
        </HTML>
    </xsl:template>
    <xsl:template name="ins-br">
        <xsl:param name="value" />
        <xsl:choose>
            <xsl:when test="contains($value,'&#10;')">
                <xsl:value-of select="normalize-space(substring-before($value,'&#10;'))" /><BR />
                <xsl:call-template name="ins-br">
                    <xsl:with-param name="value" select="substring-after($value,'&#10;')" />
                </xsl:call-template>
            </xsl:when>
            <xsl:otherwise>
                <xsl:value-of select="$value" />
            </xsl:otherwise>
        </xsl:choose>
    </xsl:template>
    <xsl:template match="/DataRoot/様式ID" />
    <xsl:template match="/DataRoot/STYLESHEET" />
</xsl:stylesheet>
