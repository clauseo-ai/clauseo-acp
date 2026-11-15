"""The pairs behind view-nested-inner's siblings: hidden text that changes, tables three deep,
a paragraph past stetdoc's wrap, and an inner table that ends its cell and is removed (the
compare stetdoc 0.9.3 refuses). Standard library only; writes into the current folder, with
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
