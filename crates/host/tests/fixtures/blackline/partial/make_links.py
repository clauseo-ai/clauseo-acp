"""The pairs behind the blackline's checks of what stetdoc 0.9.3's compare does not mark or the
view does not show: a comment whose text changes, a text box whose text changes, a table cell
whose shading changes, and a change in a footnote alone and in a header alone. Standard library
only; writes into the current folder, with make_docs.py's package writer and make_more.py's
helpers."""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
exec(open(os.path.join(HERE, 'make_docs.py')).read().split('# 1.')[0])
SRC = open(os.path.join(HERE, 'make_more.py')).read()
# make_more.py's helpers only, not its pairs: each helper's block up to its first blank line.
for name in ['def tc(', 'def tbl(', "S = '", 'def run(', 'def para(', 'SEP = (', 'XML = ',
             'GC = ', 'def cell(', 'def row(', 'def grid(', 'def noted(']:
    start = SRC.index(name)
    end = SRC.index('\n\n', start)
    exec(SRC[start:end])

COMMENTS_TYPE = f'<Override PartName="/word/comments.xml" ContentType="{CT}comments+xml"/>'


def commented(name, note):
    comments = (f'{XML}<w:comments {W}><w:comment w:id="0" w:author="Partner" w:initials="P" '
                f'w:date="2026-11-01T00:00:00Z">{p(note)}</w:comment></w:comments>')
    body = (p('Intro.') + '<w:p><w:commentRangeStart w:id="0"/>'
            '<w:r><w:t xml:space="preserve">The fee is due monthly.</w:t></w:r>'
            '<w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r></w:p>' + S)
    pkg(name, body, {'word/comments.xml': comments},
        f'<Relationship Id="rIdC" Type="{R}comments" Target="comments.xml"/>', COMMENTS_TYPE)


commented('comment-old.docx', 'Check the fee with the client.')
commented('comment-new.docx', 'The client agreed the fee.')

WPS = ('xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
       'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
       'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"')


def boxed(name, text):
    shape = (f'<w:r><w:drawing><wp:inline {WPS}><wp:extent cx="1828800" cy="457200"/>'
             '<wp:docPr id="2" name="Box"/><a:graphic><a:graphicData '
             'uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><wps:wsp>'
             '<wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/>'
             '<a:ext cx="1828800" cy="457200"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/>'
             f'</a:prstGeom></wps:spPr><wps:txbx><w:txbxContent>{p(text)}</w:txbxContent>'
             '</wps:txbx><wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline>'
             '</w:drawing></w:r>')
    body = p('Intro.') + '<w:p><w:r><w:t xml:space="preserve">See: </w:t></w:r>' + shape + '</w:p>' + S
    pkg(name, body)


boxed('textbox-old.docx', 'Cap of 1 million.')
boxed('textbox-new.docx', 'Cap of 2 million.')


def shaded(name, fill):
    tcpr = f'<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>' if fill else ''
    pkg(name, p('Intro.') + grid(row(cell(p('Price')) + cell(p('100'), tcpr)), 2) + S)


shaded('cellfmt-old.docx', None)
shaded('cellfmt-new.docx', 'FFFF00')
noted('footnote-old.docx', 'The term is one year.', 'Note one year.', 'Header.')
noted('footnote-new.docx', 'The term is one year.', 'Note two years.', 'Header.')
noted('header-old.docx', 'The term is one year.', 'Note.', 'Header one year.')
noted('header-new.docx', 'The term is one year.', 'Note.', 'Header two years.')


# A document that already holds tracked format changes of a table, its grid, a row, a cell, a
# list paragraph and the section, and a move, viewed as it is (`stetdoc view --all --json` and
# `stetdoc check --json props.docx`): the view marks the table, grid and row changes and the
# paragraph's, and neither the cell's nor the last section's.
def ch(i):
    return f'w:id="{i}" {D}'


TCP = (f'<w:shd w:val="clear" w:color="auto" w:fill="FFFF00"/><w:tcPrChange {ch(21)}><w:tcPr>'
       '<w:tcW w:w="2000" w:type="dxa"/></w:tcPr></w:tcPrChange>')
PROPS_TABLE = (
    f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:jc w:val="center"/><w:tblPrChange {ch(23)}>'
    '<w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr></w:tblPrChange></w:tblPr><w:tblGrid>'
    f'<w:gridCol w:w="3000"/><w:gridCol w:w="2000"/><w:tblGridChange {ch(24)}><w:tblGrid>'
    f'{GC}{GC}</w:tblGrid></w:tblGridChange></w:tblGrid>'
    f'<w:tr><w:trPr><w:cantSplit/><w:trPrChange {ch(22)}><w:trPr/></w:trPrChange></w:trPr>'
    f'{cell(p("Price"))}{cell(p("100"), TCP)}</w:tr></w:tbl>')
NUMBERED = (f'<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr><w:pPrChange '
            f'{ch(25)}><w:pPr/></w:pPrChange></w:pPr><w:r><w:t>Numbered now.</w:t></w:r></w:p>')
MOVED = (f'<w:p><w:moveFromRangeStart w:id="30" w:name="m1" {D}/><w:moveFrom {ch(31)}><w:r><w:t>'
         'Moved text.</w:t></w:r></w:moveFrom><w:moveFromRangeEnd w:id="30"/></w:p>'
         + p('Middle.') +
         f'<w:p><w:moveToRangeStart w:id="32" w:name="m1" {D}/><w:moveTo {ch(33)}><w:r><w:t>'
         'Moved text.</w:t></w:r></w:moveTo><w:moveToRangeEnd w:id="32"/></w:p>')
SECT = (f'<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:sectPrChange {ch(26)}><w:sectPr>'
        '<w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:sectPrChange></w:sectPr>')
NUMBERING = (f'{XML}<w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0">'
             '<w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>'
             '</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>')
pkg('props.docx', p('Intro.') + PROPS_TABLE + NUMBERED + MOVED + SECT,
    {'word/numbering.xml': NUMBERING},
    f'<Relationship Id="rIdNum" Type="{R}numbering" Target="numbering.xml"/>',
    f'<Override PartName="/word/numbering.xml" ContentType="{CT}numbering+xml"/>')
