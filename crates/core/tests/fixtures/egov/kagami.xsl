<?xml version="1.0" encoding="UTF-8"?>
<!-- テスト用の鑑の XSL。xsl:output、format-number、copy-of、属性値の組み立てを使う。 -->
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
<xsl:output method="html" encoding="UTF-8" indent="yes"/>

<xsl:template match="DOC">
	<xsl:apply-templates select="BODY"/>
</xsl:template>

<xsl:template match="BODY">
	<HTML>
	<HEAD><TITLE><xsl:value-of select="TITLE"/></TITLE></HEAD>
	<BODY>
		<P ALIGN="RIGHT"><xsl:value-of select="DOCNO"/></P>
		<P ALIGN="RIGHT" ID="date">
			<xsl:value-of select="substring(DATE,1,2)"/>
			<xsl:value-of select="format-number(substring(DATE,3,2),'##')"/>
			<xsl:value-of select="substring(DATE,5,1)"/>
			<xsl:value-of select="format-number(substring(DATE,6,2),'##')"/>
			<xsl:value-of select="substring(DATE,8,1)"/>
			<xsl:value-of select="format-number(substring(DATE,9,2),'##')"/>
			<xsl:value-of select="substring(DATE,11,1)"/>
		</P>
		<P><xsl:value-of select="TO/AFF"/></P>
		<P><xsl:value-of select="concat(TO/NAME,'　',TO/HONORIFC)"/></P>
		<P ALIGN="RIGHT"><xsl:value-of select="AUTHOR/AFF"/></P>
		<P ALIGN="CENTER"><xsl:value-of select="TITLE"/></P>
		<xsl:apply-templates select="MAINTXT"/>
		<xsl:apply-templates select="APPENDIX"/>
		<xsl:apply-templates select="MAINTXT3"/>
	</BODY>
	</HTML>
</xsl:template>

<xsl:template match="P">
	<xsl:copy-of select="."/>
</xsl:template>

<xsl:template match="APPENDIX">
	<xsl:apply-templates select="DOCLINK"/>
	<BR/>
</xsl:template>

<xsl:template match="DOCLINK">
	<a>
		<xsl:attribute name="href"><xsl:value-of select="@REF"/></xsl:attribute>
		<xsl:attribute name="target">_blank</xsl:attribute>
		<xsl:value-of select="../APPTITLE"/> ( <xsl:value-of select="@REF"/> )
	</a>
</xsl:template>

</xsl:stylesheet>
