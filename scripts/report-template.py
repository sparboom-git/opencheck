"""Writes OpenCheck's standard report template, a Word document:
crates/opencheck-app/assets/report-template.docx

Run from the repository root:  python scripts/report-template.py
"""

import zipfile

OUT = "crates/opencheck-app/assets/report-template.docx"

INK = "20252E"
MUTED = "6B7380"
RULE = "DFE3EA"
# A4 with 2 cm sides: the text is 9638 twips wide.
TEXT_WIDTH = 9638

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" ' \
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n'


def run(text, props=""):
    rpr = f"<w:rPr>{props}</w:rPr>" if props else ""
    return f'<w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r>'


def field(instr, props=""):
    rpr = f"<w:rPr>{props}</w:rPr>" if props else ""
    return (f'<w:r>{rpr}<w:fldChar w:fldCharType="begin"/></w:r>'
            f'<w:r>{rpr}<w:instrText xml:space="preserve"> {instr} </w:instrText></w:r>'
            f'<w:r>{rpr}<w:fldChar w:fldCharType="separate"/></w:r>'
            f'<w:r>{rpr}<w:t>1</w:t></w:r>'
            f'<w:r>{rpr}<w:fldChar w:fldCharType="end"/></w:r>')


def para(content, style=None, props=""):
    ppr = (f'<w:pStyle w:val="{style}"/>' if style else "") + props
    return f"<w:p><w:pPr>{ppr}</w:pPr>{content}</w:p>"


def style(kind, sid, name, ppr="", rpr="", based="Normal", extra=""):
    based_on = f'<w:basedOn w:val="{based}"/>' if based else ""
    return (f'<w:style w:type="{kind}" w:styleId="{sid}"><w:name w:val="{name}"/>{based_on}'
            f'{extra}<w:qFormat/><w:pPr>{ppr}</w:pPr><w:rPr>{rpr}</w:rPr></w:style>')


STYLES = f"""<w:styles {W}>
<w:docDefaults>
<w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="Arial" w:cs="Arial"/>
<w:color w:val="{INK}"/><w:sz w:val="19"/><w:szCs w:val="19"/><w:lang w:val="da-DK"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="264" w:lineRule="auto"/></w:pPr></w:pPrDefault>
</w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
{style("paragraph", "Title", "Title", '<w:spacing w:after="40"/>', '<w:b/><w:sz w:val="44"/>', extra='<w:next w:val="Subtitle"/>')}
{style("paragraph", "Subtitle", "Subtitle", '<w:spacing w:after="240"/>', f'<w:color w:val="{MUTED}"/><w:sz w:val="23"/>')}
{style("paragraph", "Heading1", "heading 1",
       f'<w:keepNext/><w:pBdr><w:bottom w:val="single" w:sz="8" w:space="2" w:color="{INK}"/></w:pBdr>'
       '<w:spacing w:before="360" w:after="160"/><w:outlineLvl w:val="0"/>',
       '<w:b/><w:sz w:val="27"/>', extra='<w:next w:val="Normal"/>')}
{style("paragraph", "Heading2", "heading 2",
       '<w:keepNext/><w:spacing w:before="280" w:after="120"/><w:outlineLvl w:val="1"/>',
       '<w:b/><w:sz w:val="22"/>', extra='<w:next w:val="Normal"/>')}
{style("paragraph", "Header", "header",
       f'<w:tabs><w:tab w:val="right" w:pos="{TEXT_WIDTH}"/></w:tabs><w:spacing w:after="0"/>',
       f'<w:color w:val="{MUTED}"/><w:sz w:val="16"/>')}
{style("paragraph", "Footer", "footer",
       f'<w:tabs><w:tab w:val="right" w:pos="{TEXT_WIDTH}"/></w:tabs><w:spacing w:after="0"/>',
       f'<w:color w:val="{MUTED}"/><w:sz w:val="16"/>')}
{style("paragraph", "Small", "Small", '<w:spacing w:before="320"/>', f'<w:color w:val="{MUTED}"/><w:sz w:val="16"/>')}
<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/>
<w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/>
<w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/>
</w:tblCellMar></w:tblPr></w:style>
</w:styles>"""

BODY = "".join([
    para(run("{{project}}"), "Title"),
    para(run("{{report}} · Konsistens- og kollisionskontrol"), "Subtitle"),
    para(run("{{revisions}}")),
    para(run("{{content}}")),
    para(run("Genereret af OpenCheck {{opencheck}} den {{today}} med data fra {{data_date}}. "
             "Objekter kan genfindes i Revit via deres Revit ID (Element ID, fx med Select by ID) "
             "og i andre programmer via deres IFC GlobalId; emnernes ID svarer til BCF-emnernes "
             "GUID."), "Small"),
])

SECTION = """<w:sectPr>
<w:headerReference w:type="default" r:id="rIdHeader"/>
<w:headerReference w:type="first" r:id="rIdHeaderFirst"/>
<w:footerReference w:type="default" r:id="rIdFooter"/>
<w:footerReference w:type="first" r:id="rIdFooter"/>
<w:pgSz w:w="11906" w:h="16838"/>
<w:pgMar w:top="1418" w:right="1134" w:bottom="1247" w:left="1134" w:header="624" w:footer="567" w:gutter="0"/>
<w:titlePg/>
</w:sectPr>"""

DOCUMENT = f"<w:document {W}><w:body>{BODY}{SECTION}</w:body></w:document>"

HEADER = (f"<w:hdr {W}>"
          + para(run("{{project}} – {{report}}") + "<w:r><w:tab/></w:r>" + run("Revision {{revision}}"),
                 "Header", f'<w:pBdr><w:bottom w:val="single" w:sz="4" w:space="4" w:color="{RULE}"/></w:pBdr>')
          + "</w:hdr>")
HEADER_FIRST = f"<w:hdr {W}>" + para("", "Header") + "</w:hdr>"
FOOTER = (f"<w:ftr {W}>"
          + para(run("{{company}}") + "<w:r><w:tab/></w:r>" + run("Side ") + field("PAGE")
                 + run(" af ") + field("NUMPAGES"), "Footer")
          + "</w:ftr>")

SETTINGS = f"""<w:settings {W}><w:defaultTabStop w:val="709"/>
<w:characterSpacingControl w:val="doNotCompress"/><w:compat>
<w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
</w:compat></w:settings>"""

REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
DOC_RELS = f"""<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdStyles" Type="{REL}/styles" Target="styles.xml"/>
<Relationship Id="rIdSettings" Type="{REL}/settings" Target="settings.xml"/>
<Relationship Id="rIdHeader" Type="{REL}/header" Target="header1.xml"/>
<Relationship Id="rIdHeaderFirst" Type="{REL}/header" Target="header2.xml"/>
<Relationship Id="rIdFooter" Type="{REL}/footer" Target="footer1.xml"/>
</Relationships>"""

ROOT_RELS = f"""<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
</Relationships>"""

CORE = """<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:title>{{project}} – {{report}}</dc:title><dc:creator>OpenCheck</dc:creator></cp:coreProperties>"""

WML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
TYPES = f"""<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Default Extension="jpeg" ContentType="image/jpeg"/>
<Override PartName="/word/document.xml" ContentType="{WML}.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="{WML}.styles+xml"/>
<Override PartName="/word/settings.xml" ContentType="{WML}.settings+xml"/>
<Override PartName="/word/header1.xml" ContentType="{WML}.header+xml"/>
<Override PartName="/word/header2.xml" ContentType="{WML}.header+xml"/>
<Override PartName="/word/footer1.xml" ContentType="{WML}.footer+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
</Types>"""

PARTS = [
    ("[Content_Types].xml", TYPES),
    ("_rels/.rels", ROOT_RELS),
    ("docProps/core.xml", CORE),
    ("word/document.xml", DOCUMENT),
    ("word/_rels/document.xml.rels", DOC_RELS),
    ("word/styles.xml", STYLES),
    ("word/settings.xml", SETTINGS),
    ("word/header1.xml", HEADER),
    ("word/header2.xml", HEADER_FIRST),
    ("word/footer1.xml", FOOTER),
]

with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
    for name, xml in PARTS:
        # Fixed times keep the file the same each time it is written.
        info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(info, DECL + xml.replace("\n", ""))
print("written", OUT)
