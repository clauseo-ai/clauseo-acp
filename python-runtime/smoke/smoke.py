"""The smoke test of Clauseo's Python runtime, run by `cargo xtask python-runtime build` with the
finished runtime, isolated (-I) and without writing bytecode (-B):

    <runtime python> -I -B smoke.py <runtime dir> <fixtures dir> <work dir> <pandoc version>

It checks that this is the runtime's own CPython, that every bundled package imports from the
runtime, that what the trim removed is gone, and that each library does its job: a .docx, .xlsx and
.pptx written and read back, the fixture PDF read with pypdf and pypdfium2, and a small pandas and
numpy computation. It also runs the runtime's pandoc (bin/pandoc on macOS, clauseo-bin\\pandoc.exe on
Windows): `--version` must name <pandoc version>, and the .docx goes to Markdown and back. It
writes only into <work dir>. Any failure raises and exits non-zero.
"""

import importlib
import os
import subprocess
import sys

EXPECTED_VERSION = (3, 13, 18)

# Every package the runtime bundles, by import name, and the standard modules that need a library
# PBS builds in (OpenSSL, SQLite, libffi, xz, bzip2, zlib, mpdecimal, expat).
PACKAGES = [
    "numpy", "pandas", "lxml.etree", "PIL.Image", "pypdfium2", "pypdf", "docx", "openpyxl",
    "et_xmlfile", "pptx", "xlsxwriter", "docx2python", "paragraphs", "dateutil", "tzdata",
    "typing_extensions", "pip",
]
STANDARD = ["ssl", "hashlib", "sqlite3", "ctypes", "lzma", "bz2", "zlib", "decimal", "pyexpat",
            "zoneinfo", "json", "csv", "zipfile"]
REMOVED = ["tkinter", "_tkinter", "idlelib", "turtle", "ensurepip"]


def check(condition, what):
    if not condition:
        raise SystemExit(f"smoke: FAILED: {what}")
    print(f"smoke: ok: {what}")


def same_or_inside(path, root):
    return os.path.normcase(os.path.realpath(path)).startswith(os.path.normcase(os.path.realpath(root)))


def interpreter(root):
    check(sys.version_info[:3] == EXPECTED_VERSION, f"CPython {sys.version.split()[0]}")
    check(same_or_inside(sys.prefix, root), f"sys.prefix {sys.prefix} is the runtime")
    check(sys.flags.isolated == 1, "isolated mode")


def imports(root):
    for name in PACKAGES + STANDARD:
        module = importlib.import_module(name)
        where = getattr(module, "__file__", None)
        if name in PACKAGES or where:
            check(where is None or same_or_inside(where, root), f"import {name} from the runtime")
    for name in REMOVED:
        try:
            importlib.import_module(name)
        except ImportError:
            check(True, f"{name} is removed")
        else:
            check(False, f"{name} is removed")
    import ssl
    ssl.create_default_context()
    check(ssl.OPENSSL_VERSION.startswith("OpenSSL 3"), ssl.OPENSSL_VERSION)


def docx_roundtrip(work):
    import docx
    from docx2python import docx2python

    path = os.path.join(work, "smoke.docx")
    document = docx.Document()
    document.add_heading("Clauseo smoke test", level=1)
    document.add_paragraph("The first clause.")
    table = document.add_table(rows=1, cols=2)
    table.rows[0].cells[0].text, table.rows[0].cells[1].text = "Party", "Acme Ltd"
    document.save(path)
    again = docx.Document(path)
    texts = [p.text for p in again.paragraphs]
    check("The first clause." in texts, "python-docx writes and reads a .docx")
    check(again.tables[0].rows[0].cells[1].text == "Acme Ltd", "python-docx reads the table back")
    with docx2python(path) as content:
        check("The first clause." in content.text, "docx2python reads the .docx")


def xlsx_roundtrip(work):
    import openpyxl
    import pandas
    import xlsxwriter

    first = os.path.join(work, "openpyxl.xlsx")
    book = openpyxl.Workbook()
    sheet = book.active
    sheet.append(["item", "amount"])
    sheet.append(["fee", 1200])
    sheet.append(["costs", 300])
    sheet["C1"] = "=SUM(B2:B3)"
    book.save(first)
    again = openpyxl.load_workbook(first)
    check(again.active["B2"].value == 1200, "openpyxl writes and reads a .xlsx")
    check(again.active["C1"].value == "=SUM(B2:B3)", "openpyxl keeps a formula")

    second = os.path.join(work, "xlsxwriter.xlsx")
    workbook = xlsxwriter.Workbook(second)
    worksheet = workbook.add_worksheet("Fees")
    worksheet.write_row(0, 0, ["item", "amount"])
    worksheet.write_row(1, 0, ["fee", 1500])
    workbook.close()
    check(openpyxl.load_workbook(second)["Fees"]["B2"].value == 1500, "XlsxWriter's .xlsx reads back")
    frame = pandas.read_excel(first, engine="openpyxl", usecols="A:B")
    check(int(frame["amount"].sum()) == 1500, "pandas reads the .xlsx with openpyxl")


def pptx_roundtrip(work):
    from PIL import Image
    from pptx import Presentation
    from pptx.util import Inches

    picture = os.path.join(work, "smoke.png")
    Image.new("RGB", (64, 32), (200, 30, 30)).save(picture)
    check(Image.open(picture).size == (64, 32), "Pillow writes and reads a .png")
    path = os.path.join(work, "smoke.pptx")
    deck = Presentation()
    slide = deck.slides.add_slide(deck.slide_layouts[1])
    slide.shapes.title.text = "Clauseo smoke test"
    slide.shapes.add_picture(picture, Inches(1), Inches(1))
    deck.save(path)
    again = Presentation(path)
    titles = [s.shapes.title.text for s in again.slides]
    check(titles == ["Clauseo smoke test"], "python-pptx writes and reads a .pptx")


def pdf_read(fixtures):
    import pypdf
    import pypdfium2

    path = os.path.join(fixtures, "hello.pdf")
    reader = pypdf.PdfReader(path)
    check("Clauseo smoke test" in reader.pages[0].extract_text(), "pypdf extracts the PDF's text")
    pdf = pypdfium2.PdfDocument(path)
    page = pdf[0]
    text = page.get_textpage().get_text_range()
    check("Clauseo smoke test" in text, "pypdfium2 extracts the PDF's text")
    image = page.render(scale=1).to_pil()
    check(image.size[0] > 100 and image.size[1] > 100, "pypdfium2 renders the page with Pillow")
    pdf.close()


def numbers():
    import datetime
    import zoneinfo

    import numpy
    import pandas
    from dateutil import parser
    from lxml import etree

    matrix = numpy.array([[3.0, 1.0], [1.0, 2.0]])
    solution = numpy.linalg.solve(matrix, numpy.array([9.0, 8.0]))
    check(numpy.allclose(solution, [2.0, 3.0]), "numpy solves a linear system")
    frame = pandas.DataFrame({"party": ["a", "b", "a"], "amount": [1, 2, 3]})
    check(frame.groupby("party")["amount"].sum().to_dict() == {"a": 4, "b": 2}, "pandas groups")
    when = parser.parse("8 November 2026 10:00").replace(tzinfo=zoneinfo.ZoneInfo("Europe/London"))
    check(when.utcoffset() == datetime.timedelta(0), "dateutil and zoneinfo (tzdata)")
    tree = etree.fromstring(b"<doc><clause n='1'>one</clause><clause n='2'>two</clause></doc>")
    check(tree.xpath("string(//clause[@n='2'])") == "two", "lxml parses and runs XPath")
    xslt = etree.XSLT(etree.fromstring(
        b"<xsl:stylesheet version='1.0' xmlns:xsl='http://www.w3.org/1999/XSL/Transform'>"
        b"<xsl:template match='/'><n><xsl:value-of select='count(//clause)'/></n></xsl:template>"
        b"</xsl:stylesheet>"))
    check(str(xslt(tree)).strip().endswith("<n>2</n>"), "lxml runs XSLT (libxslt)")


def pandoc(root, work, version):
    import docx

    exe = os.path.join(root, "clauseo-bin", "pandoc.exe") if os.name == "nt" else os.path.join(root, "bin", "pandoc")
    check(os.path.isfile(exe), f"pandoc is at {exe}")

    def run(*args):
        done = subprocess.run([exe, *args], cwd=work, capture_output=True, text=True, timeout=120)
        if done.returncode != 0:
            raise SystemExit(f"smoke: FAILED: pandoc {' '.join(args)}: {done.stderr.strip()}")
        return done.stdout

    first = run("--version").splitlines()[0]
    check(first == f"pandoc {version}", f"{first} (want pandoc {version})")
    # smoke.docx is the file docx_roundtrip wrote with python-docx.
    markdown = run("smoke.docx", "--from", "docx", "--to", "gfm")
    check("The first clause." in markdown and "Acme Ltd" in markdown, "pandoc reads the .docx to Markdown")
    with open(os.path.join(work, "smoke.md"), "w", encoding="utf-8") as out:
        out.write(markdown)
    run("smoke.md", "--from", "gfm", "--to", "docx", "--output", "back.docx")
    back = docx.Document(os.path.join(work, "back.docx"))
    texts = [p.text for p in back.paragraphs]
    check("The first clause." in texts, "pandoc writes the Markdown back to a .docx python-docx reads")
    check(back.tables and back.tables[0].rows[0].cells[1].text == "Acme Ltd", "the table survives the round trip")


def main():
    if len(sys.argv) != 5:
        raise SystemExit("usage: smoke.py <runtime dir> <fixtures dir> <work dir> <pandoc version>")
    root, fixtures, work, version = sys.argv[1:5]
    interpreter(root)
    imports(root)
    docx_roundtrip(work)
    pandoc(root, work, version)
    xlsx_roundtrip(work)
    pptx_roundtrip(work)
    pdf_read(fixtures)
    numbers()
    print("smoke: all passed")


if __name__ == "__main__":
    main()
