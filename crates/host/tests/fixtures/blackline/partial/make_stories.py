"""The pairs behind the blackline's notes, headers and footers, and its table formatting: a
header in only one version beside a body change, footnotes, endnotes, headers and footers that
change across two sections, and a cell's shading change beside a body change. Standard library
only; writes into the current folder, with make_docs.py's package writer and make_more.py's
helpers."""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
exec(open(os.path.join(HERE, 'make_docs.py')).read().split('# 1.')[0])
SRC = open(os.path.join(HERE, 'make_more.py')).read()
# make_more.py's helpers only, not its pairs: each helper's block up to its first blank line.
for name in ["S = '", 'SEP = (', 'XML = ', 'GC = ', 'def cell(', 'def row(', 'def grid(']:
    start = SRC.index(name)
    end = SRC.index('\n\n', start)
    exec(SRC[start:end])


def unmatched(name, body_text, header):
    """A body paragraph, and a default header only when `header` is given."""
    if header is None:
        pkg(name, p(body_text) + S)
        return
    parts = {'word/header1.xml': f'{XML}<w:hdr {W}>{p(header)}</w:hdr>'}
    rels = f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
    types = f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
    body = (p(body_text) +
            '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>')
    pkg(name, body, parts, rels, types)


# A header in the newer version only (stetdoc's compare warns `compare_story_unmatched` and
# marks nothing for it), beside a body change.
unmatched('unmatched-old.docx', 'The term is one year.', None)
unmatched('unmatched-new.docx', 'The term is two years.', 'Confidential.')


def stories(name, word):
    """Two sections: a footnote and an endnote, a default header both sections use, and a
    first-page footer in the second; each holds `word`."""
    endnote_sep = SEP.replace('footnote', 'endnote')
    parts = {
        'word/footnotes.xml': (f'{XML}<w:footnotes {W}>{SEP}<w:footnote w:id="1">'
                               f'{p("Footnote " + word + ".")}</w:footnote></w:footnotes>'),
        'word/endnotes.xml': (f'{XML}<w:endnotes {W}>{endnote_sep}<w:endnote w:id="1">'
                              f'{p("Endnote " + word + ".")}</w:endnote></w:endnotes>'),
        'word/header1.xml': f'{XML}<w:hdr {W}>{p("Header " + word + ".")}</w:hdr>',
        'word/footer1.xml': f'{XML}<w:ftr {W}>{p("First footer " + word + ".")}</w:ftr>',
    }
    rels = (f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
            f'<Relationship Id="rIdF" Type="{R}footer" Target="footer1.xml"/>'
            f'<Relationship Id="rIdN" Type="{R}footnotes" Target="footnotes.xml"/>'
            f'<Relationship Id="rIdE" Type="{R}endnotes" Target="endnotes.xml"/>')
    types = (f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
             f'<Override PartName="/word/footer1.xml" ContentType="{CT}footer+xml"/>'
             f'<Override PartName="/word/footnotes.xml" ContentType="{CT}footnotes+xml"/>'
             f'<Override PartName="/word/endnotes.xml" ContentType="{CT}endnotes+xml"/>')
    body = ('<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdH"/>'
            '</w:sectPr></w:pPr><w:r><w:t xml:space="preserve">Part one.</w:t></w:r>'
            '<w:r><w:footnoteReference w:id="1"/></w:r></w:p>'
            '<w:p><w:r><w:t xml:space="preserve">Part two.</w:t></w:r>'
            '<w:r><w:endnoteReference w:id="1"/></w:r></w:p>'
            '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/>'
            '<w:footerReference w:type="first" r:id="rIdF"/><w:titlePg/></w:sectPr>')
    pkg(name, body, parts, rels, types)


stories('stories-old.docx', 'one')
stories('stories-new.docx', 'two')


def shaded(name, fill, body_text):
    """A body paragraph, then a 2-cell table whose second cell is shaded `fill` when given."""
    tcpr = f'<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>' if fill else ''
    pkg(name, p(body_text) + grid(row(cell(p('Price')) + cell(p('100'), tcpr)), 2) + S)


# The cell shading of cellfmt-*.docx (make_links.py), beside a body change: stetdoc's compare
# marks the body's change and nothing for the shading.
shaded('cellfmt-body-old.docx', None, 'The term is one year.')
shaded('cellfmt-body-new.docx', 'FFFF00', 'The term is two years.')

# A paragraph that ends a section, inserted: stetdoc's compare can't mark it up, leaves it as the
# newer version has it and lists it (`compare_not_redlined`, its receipt's exclusions).
LONG = 'The tenant shall keep the premises in good repair throughout the term of this lease.'
pkg('excluded-old.docx', p(LONG) + S)
pkg('excluded-new.docx', p(LONG) + '<w:p><w:pPr><w:sectPr><w:pgSz w:w="11906" w:h="16838"/>'
    '</w:sectPr></w:pPr><w:r><w:t>End of the section</w:t></w:r></w:p>' + S)
