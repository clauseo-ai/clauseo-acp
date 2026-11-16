"""The pairs behind view-nested-inner's siblings: hidden text that changes, tables three deep,
a paragraph past stetdoc's wrap, and an inner table that ends its cell and is removed (the
compare stetdoc 0.9.3 refuses), a sentence struck through and one hidden as tracked format
changes, a changed footnote and header, and documents that already hold a tracked row
insertion, a row deletion, a cell insertion and deletion, and a row inserted in a nested table
(viewed as they are, with no compare); and pairs where a signature picture is inserted and
where only a link's address changes. Standard library only; writes into the current folder, with
make_docs.py's package writer (run make_docs.py's pairs from a temporary folder: it writes
other files too)."""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
exec(open(os.path.join(HERE, 'make_docs.py')).read().split('# 1.')[0])


def tc(x):
    return f'<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>{x}</w:tc>'


def tbl(rows, cols=1):
    grid = ''.join('<w:gridCol w:w="2000"/>' for _ in range(cols))
    body = ''.join('<w:tr>' + ''.join(tc(c) for c in r) + '</w:tr>' for r in rows)
    return (f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>'
            f'<w:tblGrid>{grid}</w:tblGrid>{body}</w:tbl>')


S = '<w:sectPr/>'


def hid(t):
    return ('<w:p><w:r><w:t xml:space="preserve">Visible </w:t></w:r>'
            f'<w:r><w:rPr><w:vanish/></w:rPr><w:t>{t}</w:t></w:r></w:p>')


pkg('hidden-change-old.docx', p('Intro.') + hid('hidden one year') + S)
pkg('hidden-change-new.docx', p('Intro.') + hid('hidden two years') + S)


def d3(t):
    return tbl([[p('L1.') + tbl([[p('L2.') + tbl([[p(t)]]) + p('')]]) + p('')]])


pkg('deep3-old.docx', p('Intro.') + d3('Deep one year.') + S)
pkg('deep3-new.docx', p('Intro.') + d3('Deep two years.') + S)
LONG = ' '.join('word%d' % i for i in range(600))
pkg('long-old.docx', p('Intro one.') + p(LONG) + S)
pkg('long-new.docx', p('Intro two.') + p(LONG + ' end') + S)
# The inner table is the last thing in its cell, and the new version removes it.
pkg('inner-last-old.docx', p('Intro.') + tbl([[p('Outer.') + tbl([[p('Inner.')]])]]) + p('End.') + S)
pkg('inner-last-new.docx', p('Intro.') + tbl([[p('Outer.')]]) + p('End.') + S)


def run(t, rpr=''):
    props = f'<w:rPr>{rpr}</w:rPr>' if rpr else ''
    return f'<w:r>{props}<w:t xml:space="preserve">{t}</w:t></w:r>'


def para(*runs):
    return '<w:p>' + ''.join(runs) + '</w:p>'


# A tracked format change only: struck through, and hidden.
pkg('strike-old.docx', p('Intro.') + para(run('Keep this. '), run('Strike this sentence.')) + S)
pkg('strike-new.docx', p('Intro.') + para(run('Keep this. '), run('Strike this sentence.', '<w:strike/>')) + S)
pkg('hide-old.docx', p('Intro.') + para(run('Keep this. '), run('Hide this sentence.')) + S)
pkg('hide-new.docx', p('Intro.') + para(run('Keep this. '), run('Hide this sentence.', '<w:vanish/>')) + S)
# A footnote and a header that change with the body: stetdoc counts 6 changes, the body shows 2.
SEP = ('<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
       '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/>'
       '</w:r></w:p></w:footnote>')
XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'


def noted(name, body_text, note, header):
    parts = {
        'word/footnotes.xml': f'{XML}<w:footnotes {W}>{SEP}<w:footnote w:id="1">{p(note)}</w:footnote></w:footnotes>',
        'word/header1.xml': f'{XML}<w:hdr {W}>{p(header)}</w:hdr>',
    }
    rels = (f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
            f'<Relationship Id="rIdN" Type="{R}footnotes" Target="footnotes.xml"/>')
    types = (f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
             f'<Override PartName="/word/footnotes.xml" ContentType="{CT}footnotes+xml"/>')
    body = (f'<w:p><w:r><w:t xml:space="preserve">{body_text}</w:t></w:r>'
            '<w:r><w:footnoteReference w:id="1"/></w:r></w:p>'
            '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>')
    pkg(name, body, parts, rels, types)


noted('notes-old.docx', 'The term is one year.', 'Note one year.', 'Header one year.')
noted('notes-new.docx', 'The term is two years.', 'Note two years.', 'Header two years.')


# Documents that already hold a tracked table change, viewed as they are (`stetdoc view --all
# --json <name>.docx`): a row inserted or deleted (w:trPr/w:ins, w:del), a cell inserted or
# deleted (w:tcPr/w:cellIns, w:cellDel), and a row inserted in a table inside a cell.
GC = '<w:gridCol w:w="2000"/>'


def cell(x, tcpr=''):
    return f'<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/>{tcpr}</w:tcPr>{x}</w:tc>'


def row(cells, trpr=''):
    props = f'<w:trPr>{trpr}</w:trPr>' if trpr else ''
    return f'<w:tr>{props}{cells}</w:tr>'


def grid(rows, cols):
    return (f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>'
            f'<w:tblGrid>{GC * cols}</w:tblGrid>{rows}</w:tbl>')


KEPT = row(cell(p('Kept A.')) + cell(p('Kept B.')))
pkg('row-ins.docx', p('Intro.') + grid(KEPT + row(cell(p('New A.')) + cell(p('New B.')), f'<w:ins w:id="11" {D}/>'), 2) + S)
pkg('row-del.docx', p('Intro.') + grid(KEPT + row(cell(p('Gone A.')) + cell(p('Gone B.')), f'<w:del w:id="12" {D}/>'), 2) + S)
pkg('cell-ins.docx', p('Intro.') + grid(row(cell(p('Kept A.')) + cell(p('New cell.'), f'<w:cellIns w:id="13" {D}/>')), 2) + S)
pkg('cell-del.docx', p('Intro.') + grid(row(cell(p('Kept A.')) + cell(p('Gone cell.'), f'<w:cellDel w:id="14" {D}/>')), 2) + S)
INNER = grid(row(cell(p('Inner kept.'))) + row(cell(p('Inner new.')), f'<w:ins w:id="15" {D}/>'), 1)
pkg('row-ins-nested.docx', p('Intro.') + grid(row(cell(p('Outer.') + INNER + p(''))), 1) + S)


# A signature picture inserted after "Signed: " (stetdoc prints `{++{img}++}{#0 C}`), and a link
# whose address alone changes (stetdoc marks no change at all).
import base64  # noqa: E402

PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==')
DRAW_NS = ('xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
           'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
           'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"')
DRAWING = (f'<w:r><w:drawing><wp:inline {DRAW_NS}><wp:extent cx="914400" cy="914400"/>'
           '<wp:docPr id="1" name="Signature"/><a:graphic><a:graphicData '
           'uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr>'
           '<pic:cNvPr id="1" name="sig.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill>'
           '<a:blip r:embed="rIdImg"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr>'
           '<a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm>'
           '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData>'
           '</a:graphic></wp:inline></w:drawing></w:r>')


def signed(name, with_picture):
    body = (p('Intro.') + '<w:p><w:r><w:t xml:space="preserve">Signed: </w:t></w:r>'
            + (DRAWING if with_picture else '') + '</w:p>' + S)
    pkg(name, body, {'word/media/sig.png': PNG},
        f'<Relationship Id="rIdImg" Type="{R}image" Target="media/sig.png"/>',
        '<Default Extension="png" ContentType="image/png"/>')


signed('picture-old.docx', False)
signed('picture-new.docx', True)


def linked(name, target):
    rel = f'<Relationship Id="rIdL" Type="{R}hyperlink" Target="{target}" TargetMode="External"/>'
    body = (p('Intro.') + '<w:p><w:r><w:t xml:space="preserve">Pay to </w:t></w:r>'
            '<w:hyperlink r:id="rIdL"><w:r><w:t>the account</w:t></w:r></w:hyperlink></w:p>' + S)
    pkg(name, body, None, rel)


linked('link-old.docx', 'https://bank.example/acct-111')
linked('link-new.docx', 'https://bank.example/acct-999')
