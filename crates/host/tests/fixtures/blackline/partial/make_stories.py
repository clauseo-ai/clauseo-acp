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


def sectioned(name, cover, fill, body_text):
    """A header with a 1-cell table (its cell shaded `fill` when given) on the last section, and,
    when `cover` is given, a first section before it with a header of its own reading `cover`."""
    tcpr = f'<w:shd w:val="clear" w:color="auto" w:fill="{fill}"/>' if fill else ''
    logo = grid(row(cell(p('Logo'), tcpr)), 1)
    parts = {'word/header1.xml': f'{XML}<w:hdr {W}>{logo}{p("")}</w:hdr>'}
    rels = f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
    types = f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
    body = ''
    if cover is not None:
        parts['word/header2.xml'] = f'{XML}<w:hdr {W}>{p(cover)}</w:hdr>'
        rels += f'<Relationship Id="rIdC" Type="{R}header" Target="header2.xml"/>'
        types += f'<Override PartName="/word/header2.xml" ContentType="{CT}header+xml"/>'
        body += ('<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdC"/>'
                 '</w:sectPr></w:pPr><w:r><w:t>Cover.</w:t></w:r></w:p>')
    body += (p(body_text) +
             '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>')
    pkg(name, body, parts, rels, types)


# A cover section inserted before the only one, with a header of its own, while the shading of
# the table in the old section's header changes, beside a body change. stetdoc pairs the
# sections from the last one back: the header with the table pairs with itself.
sectioned('cover-old.docx', None, None, 'The term is one year.')
sectioned('cover-new.docx', 'Cover header.', 'FFFF00', 'The term is two years.')


def split(name, first_header, body_text):
    """"Part one." then `body_text`, the last section's header reading "Main header."; with
    `first_header`, "Part one." ends a first section of its own, whose header reads it."""
    parts = {'word/header1.xml': f'{XML}<w:hdr {W}>{p("Main header.")}</w:hdr>'}
    rels = f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
    types = f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
    first = p('Part one.')
    if first_header is not None:
        parts['word/header2.xml'] = f'{XML}<w:hdr {W}>{p(first_header)}</w:hdr>'
        rels += f'<Relationship Id="rIdS" Type="{R}header" Target="header2.xml"/>'
        types += f'<Override PartName="/word/header2.xml" ContentType="{CT}header+xml"/>'
        first = ('<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdS"/>'
                 '</w:sectPr></w:pPr><w:r><w:t>Part one.</w:t></w:r></w:p>')
    body = (first + p(body_text) +
            '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>')
    pkg(name, body, parts, rels, types)


# A section added in the newer version (an existing paragraph now ends a first section) with a
# header of its own, beside a body change. stetdoc pairs the sections from the last one back,
# so the new first section's header pairs with nothing and is not compared.
split('added-old.docx', None, 'The term is one year.')
split('added-new.docx', 'Schedule header.', 'The term is two years.')


NUMBERING = (f'{XML}<w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0">'
             '<w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>'
             '</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>')


def headed(name, header_xml):
    """A body paragraph that stays, and a default header holding `header_xml`."""
    parts = {'word/header1.xml': f'{XML}<w:hdr {W}>{header_xml}</w:hdr>',
             'word/numbering.xml': NUMBERING}
    rels = (f'<Relationship Id="rIdH" Type="{R}header" Target="header1.xml"/>'
            f'<Relationship Id="rIdNum" Type="{R}numbering" Target="numbering.xml"/>')
    types = (f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
             f'<Override PartName="/word/numbering.xml" ContentType="{CT}numbering+xml"/>')
    body = (p('Body.') +
            '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>')
    pkg(name, body, parts, rels, types)


# Headers whose text the views write differently from the part's w:t: an entity and a character
# reference, a list number, a symbol (w:sym). Each changes from "Draft 1" to "Draft 2".
FORMS = {
    'entity': lambda d: ('<w:p><w:r><w:t xml:space="preserve">Smith &amp; Co &#8212; Draft '
                         f'{d}</w:t></w:r></w:p>'),
    'numbered': lambda d: ('<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/>'
                           f'</w:numPr></w:pPr><w:r><w:t>Draft {d}</w:t></w:r></w:p>'),
    'symbol': lambda d: ('<w:p><w:r><w:sym w:font="Wingdings" w:char="F0FC"/>'
                         f'<w:t xml:space="preserve"> Draft {d}</w:t></w:r></w:p>'),
}
for form, header in FORMS.items():
    headed(f'{form}-old.docx', header(1))
    headed(f'{form}-new.docx', header(2))
