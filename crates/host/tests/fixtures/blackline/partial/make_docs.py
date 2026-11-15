import zipfile, sys
W='xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
def pkg(path, body, parts=None, rels_extra='', ct_extra=''):
    parts = parts or {}
    ct = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
      '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
      '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'+ct_extra+'</Types>')
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
      '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
    drels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'+rels_extra+'</Relationships>')
    doc = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}</w:body></w:document>'
    with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as z:
        z.writestr('[Content_Types].xml',ct); z.writestr('_rels/.rels',rels); z.writestr('word/_rels/document.xml.rels',drels); z.writestr('word/document.xml',doc)
        for n,x in parts.items(): z.writestr(n,x)
R='http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
CT='application/vnd.openxmlformats-officedocument.wordprocessingml.'
D='w:author="Reviewer" w:date="2026-11-01T00:00:00Z"'
def p(t): return f'<w:p><w:r><w:t xml:space="preserve">{t}</w:t></w:r></w:p>'
# 1. tracked changes in body, header, footer, footnote, endnote, plus rPrChange and pPrChange in the body
hdr=f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr {W}><w:p><w:r><w:t xml:space="preserve">Header </w:t></w:r><w:ins w:id="11" {D}><w:r><w:t>inserted</w:t></w:r></w:ins></w:p></w:hdr>'
ftr=f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:ftr {W}><w:p><w:r><w:t xml:space="preserve">Footer </w:t></w:r><w:del w:id="12" {D}><w:r><w:delText>deleted</w:delText></w:r></w:del></w:p></w:ftr>'
sep='<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
fn=f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes {W}>{sep}<w:footnote w:id="1"><w:p><w:r><w:t xml:space="preserve">Note </w:t></w:r><w:ins w:id="13" {D}><w:r><w:t>added</w:t></w:r></w:ins></w:p></w:footnote></w:footnotes>'
en=f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:endnotes {W}>{sep.replace("footnote","endnote")}<w:endnote w:id="1"><w:p><w:r><w:t xml:space="preserve">End </w:t></w:r><w:ins w:id="14" {D}><w:r><w:t>added</w:t></w:r></w:ins></w:p></w:endnote></w:endnotes>'
body=(f'<w:p><w:r><w:t xml:space="preserve">Body </w:t></w:r><w:ins w:id="1" {D}><w:r><w:t>inserted</w:t></w:r></w:ins>'
  f'<w:r><w:rPr><w:b/><w:rPrChange w:id="2" {D}><w:rPr/></w:rPrChange></w:rPr><w:t xml:space="preserve"> bolded</w:t></w:r>'
  f'<w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="1"/></w:r></w:p>'
  f'<w:p><w:pPr><w:jc w:val="center"/><w:pPrChange w:id="3" {D}><w:pPr/></w:pPrChange></w:pPr><w:r><w:t>Centred paragraph.</w:t></w:r></w:p>'
  '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/><w:footerReference w:type="default" r:id="rIdF"/></w:sectPr>')
pkg('tracked-nonbody-and-format.docx', body,
  {'word/header1.xml':hdr,'word/footer1.xml':ftr,'word/footnotes.xml':fn,'word/endnotes.xml':en},
  f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/><Relationship Id="rIdF" Type="{R}footer" Target="footer1.xml"/><Relationship Id="rIdN" Type="{R}footnotes" Target="footnotes.xml"/><Relationship Id="rIdE" Type="{R}endnotes" Target="endnotes.xml"/>',
  f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/><Override PartName="/word/footer1.xml" ContentType="{CT}footer+xml"/><Override PartName="/word/footnotes.xml" ContentType="{CT}footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="{CT}endnotes+xml"/>')
# 2. nested table: a table inside a table cell; old and new differ in a body paragraph
def tc(x): return f'<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>{x}</w:tc>'
inner=f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/></w:tblGrid><w:tr>{tc(p("Inner cell."))}</w:tr></w:tbl>'
outer=lambda: f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid><w:tr>{tc(p("Outer left.")+inner+p(""))}{tc(p("Outer right."))}</w:tr></w:tbl>'
pkg('nested-table-old.docx', p('The term is one year.')+outer()+p('Last paragraph.')+'<w:sectPr/>')
pkg('nested-table-new.docx', p('The term is two years.')+outer()+p('Last paragraph.')+'<w:sectPr/>')
# 3. a stetdoc view warning (hidden text): old and new differ in one paragraph
hid='<w:p><w:r><w:t xml:space="preserve">Visible </w:t></w:r><w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden words</w:t></w:r></w:p>'
pkg('hidden-text-old.docx', p('The term is one year.')+hid+'<w:sectPr/>')
pkg('hidden-text-new.docx', p('The term is two years.')+hid+'<w:sectPr/>')
