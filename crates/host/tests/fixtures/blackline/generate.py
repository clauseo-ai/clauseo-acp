#!/usr/bin/env python3
"""Generates the Word blackline fixtures in this folder (see README.md).

Run from anywhere, with stetdoc 0.9.0 on PATH:

    python3 generate.py

Needs Python 3.9 or later and nothing outside the standard library. It takes the package of
`../hindi/patra.docx` (styles, numbering, settings) as the template, replaces the body of
`word/document.xml` and adds a footnotes part, and writes `old.docx` and `new.docx`. Then it
runs `stetdoc compare --json` and `stetdoc view --all --json` on them, by the author `Clauseo`
on a fixed date, and keeps their stdout as `compare.json` and `view.json`. The redline itself
is written to a temporary folder and not kept.
"""

import io
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

HERE = Path(__file__).resolve().parent
TEMPLATE = HERE.parent / "hindi" / "patra.docx"
DATE = "2026-11-09T10:00:00"
AUTHOR = "Clauseo"

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


class Xml(str):
    """Markup made here, which `para` takes as it is."""


def run(text, bold=False, footnote=None):
    """One run's markup: text (tabs become w:tab), bold or not, or a footnote reference."""
    props = "<w:rPr><w:b/></w:rPr>" if bold else ""
    if footnote is not None:
        return Xml(
            '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>'
            f'<w:footnoteReference w:id="{footnote}"/></w:r>'
        )
    parts = []
    for n, piece in enumerate(text.split("\t")):
        if n:
            parts.append("<w:tab/>")
        if piece:
            parts.append(f'<w:t xml:space="preserve">{escape(piece)}</w:t>')
    return Xml(f"<w:r>{props}{''.join(parts)}</w:r>")


def para(*runs, style=None, num=None):
    """One paragraph of runs (a plain str is a plain run), with a style and a list number."""
    props = ""
    if style or num:
        props = "<w:pPr>"
        if style:
            props += f'<w:pStyle w:val="{style}"/>'
        if num:
            props += f'<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num}"/></w:numPr>'
        props += "</w:pPr>"
    body = "".join(r if isinstance(r, Xml) else run(r) for r in runs)
    return f"<w:p>{props}{body}</w:p>"


def table(rows):
    """A plain table of one paragraph per cell."""
    cells = lambda row: "".join(
        f'<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr>{para(c)}</w:tc>' for c in row
    )
    body = "".join(f"<w:tr>{cells(row)}</w:tr>" for row in rows)
    grid = '<w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>'
    return f'<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>{grid}{body}</w:tbl>'


def body(new):
    """The document's body blocks, old or new."""
    blocks = [
        para("Lease Agreement", style="Heading1"),
        para(
            "The Tenant shall pay rent of $1,200 each month in advance."
            if new
            else "The Tenant shall pay rent of $1,000 monthly."
        ),
    ]
    if not new:
        blocks.append(para("The deposit is returned within 30 days."))
    blocks += [
        para("Governing law is Delaware." if new else "Governing law is New York."),
        para("Pay on time.", num=9),
        para("Keep the premises clean and tidy." if new else "Keep the premises clean.", num=9),
        table([["Rent", "$1,200" if new else "$1,000"], ["Term", "12 months"]]),
        para("A notice period applies.", run("", footnote=1)),
        para(
            "Odd {curly} and back\\slash and\ttab here."
            if new
            else "Odd {braces} and back\\slash and\ttab here."
        ),
        para("This clause is ", run("important", bold=new), "."),
    ]
    if new:
        blocks.append(para("Notices go by email."))
    blocks.append(para("Signatures", style="Heading2"))
    return "".join(blocks)


def footnotes(new):
    """The footnotes part: the two separators and footnote 1."""
    text = "Sixty days." if new else "Thirty days."
    return (
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>"
        f'<w:footnotes xmlns:w="{W}" xmlns:r="{R}">'
        '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p>'
        "</w:footnote>"
        '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r>'
        "<w:continuationSeparator/></w:r></w:p></w:footnote>"
        '<w:footnote w:id="1"><w:p>'
        '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteRef/></w:r>'
        f"{run(' ' + text)}</w:p></w:footnote>"
        "</w:footnotes>"
    ).encode()


def document(new):
    """The docx bytes, old or new."""
    source = zipfile.ZipFile(TEMPLATE)
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as target:
        for info in source.infolist():
            data = source.read(info.filename)
            if info.filename == "word/document.xml":
                text = data.decode()
                blocks = body(new)
                text = re.sub(
                    r"<w:body>.*?(<w:sectPr)",
                    lambda m: f"<w:body>{blocks}{m.group(1)}",
                    text,
                    flags=re.S,
                )
                data = text.encode()
            elif info.filename == "word/_rels/document.xml.rels":
                rel = (
                    '<Relationship Id="rId90" Type="http://schemas.openxmlformats.org/'
                    'officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>'
                )
                data = data.decode().replace("</Relationships>", rel + "</Relationships>").encode()
            elif info.filename == "[Content_Types].xml":
                override = (
                    '<Override PartName="/word/footnotes.xml" ContentType="application/'
                    'vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>'
                )
                data = data.decode().replace("</Types>", override + "</Types>").encode()
            fixed = zipfile.ZipInfo(info.filename, date_time=(2024, 3, 17, 0, 0, 0))
            fixed.compress_type = zipfile.ZIP_DEFLATED
            target.writestr(fixed, data)
        fixed = zipfile.ZipInfo("word/footnotes.xml", date_time=(2024, 3, 17, 0, 0, 0))
        fixed.compress_type = zipfile.ZIP_DEFLATED
        target.writestr(fixed, footnotes(new))
    return out.getvalue()


def stetdoc(*args):
    """stetdoc's stdout for args; any exit but 0 stops the script."""
    done = subprocess.run(["stetdoc", *args], capture_output=True, text=True, check=False)
    if done.returncode != 0:
        sys.exit(f"stetdoc {' '.join(args)} exited {done.returncode}: {done.stdout}{done.stderr}")
    return done.stdout


def main():
    old, new = HERE / "old.docx", HERE / "new.docx"
    old.write_bytes(document(False))
    new.write_bytes(document(True))
    with tempfile.TemporaryDirectory() as tmp:
        redline = Path(tmp) / "redline.docx"
        compare = stetdoc(
            "compare", "--json", "--author", AUTHOR, "--date", DATE,
            "-o", str(redline), str(old), str(new),
        )
        # The redline's path is a temporary one: the fixture names it as the host would.
        (HERE / "compare.json").write_text(compare.replace(str(redline), "redline.docx"))
        view = stetdoc("view", "--all", "--json", str(redline))
        (HERE / "view.json").write_text(view.replace(str(redline), "redline.docx"))
    for name in ("old.docx", "new.docx", "compare.json", "view.json"):
        print(f"{name}: {(HERE / name).stat().st_size} bytes")


if __name__ == "__main__":
    main()
