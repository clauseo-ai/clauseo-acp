"""The smoke test of Clauseo's Python runtime, run by `cargo xtask python-runtime build` with the
finished runtime, isolated (-I) and without writing bytecode (-B):

    <runtime python> -I -B smoke.py <runtime dir> <fixtures dir> <work dir> <pandoc version> \
        <tesseract version>

It checks that this is the runtime's own CPython, that every bundled package imports from the
runtime, that what the trim removed is gone, and that each library does its job: a .docx, .xlsx and
.pptx written and read back, the fixture PDF read with pypdf and pypdfium2, and a small pandas and
numpy computation. It also runs the runtime's pandoc (bin/pandoc on macOS, clauseo-bin\\pandoc.exe on
Windows): `--version` must name <pandoc version>, and the .docx goes to Markdown and back. And it
runs the runtime's tesseract (bin/tesseract, clauseo-bin\\tesseract.exe): `--version` must name
<tesseract version> and no optional library, the models in tessdata/ beside it must be listed, and
a page it draws with Pillow (English lines and one Hindi word) must come back as text, read from a
PGM file and from stdin. It writes only into <work dir>. Any failure raises and exits non-zero.
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


# The page tesseract reads: English lines in Pillow's own font, and a Hindi word (kamal, lotus)
# of plain letters only, so it draws right without complex text shaping.
OCR_LINES = ["Clauseo smoke test", "The quick brown fox jumps over the lazy dog 1234."]
OCR_HINDI = "\u0915\u092e\u0932"
# Devanagari fonts each OS ships: macOS's Kohinoor and Devanagari MT, Windows' Nirmala UI and
# Mangal, and Noto on Linux (a local try only; Linux is not a target).
DEVANAGARI_FONTS = [
    "/System/Library/Fonts/Kohinoor.ttc",
    "/System/Library/Fonts/Supplemental/DevanagariMT.ttc",
    "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
    os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts", "Nirmala.ttc"),
    os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts", "Nirmala.ttf"),
    os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts", "mangal.ttf"),
    "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansDevanagari-Regular.ttf",
]
# Libraries tesseract --version names when it was built with them; the runtime's has none.
OPTIONAL_LIBRARIES = ["libarchive", "libcurl", "libpng", "libjpeg", "libtiff", "libwebp",
                      "libopenjp2", "zlib", "giflib", "OpenCL"]


def ocr_page(work):
    from PIL import Image, ImageDraw, ImageFont

    font_path = next((p for p in DEVANAGARI_FONTS if os.path.isfile(p)), None)
    check(font_path is not None, f"a Devanagari font for the OCR page ({font_path})")
    latin = ImageFont.load_default(size=40)
    hindi = ImageFont.truetype(font_path, 64)
    page = Image.new("L", (1400, 400), 255)
    draw = ImageDraw.Draw(page)
    for i, line in enumerate(OCR_LINES):
        draw.text((60, 50 + 70 * i), line, font=latin, fill=0)
    draw.text((60, 230), OCR_HINDI, font=hindi, fill=0)
    path = os.path.join(work, "ocr.pgm")
    page.save(path)
    return path


def tesseract(root, work, version):
    folder = os.path.join(root, "clauseo-bin" if os.name == "nt" else "bin")
    exe = os.path.join(folder, "tesseract.exe" if os.name == "nt" else "tesseract")
    tessdata = os.path.join(folder, "tessdata")
    check(os.path.isfile(exe), f"tesseract is at {exe}")
    env = {k: v for k, v in os.environ.items() if k not in ("TESSDATA_PREFIX", "OMP_THREAD_LIMIT")}
    env["OMP_THREAD_LIMIT"] = "1"

    def run(*args, stdin=None):
        done = subprocess.run([exe, *args], cwd=work, input=stdin, capture_output=True,
                              timeout=300, env=env)
        if done.returncode != 0:
            err = done.stderr.decode("utf-8", "replace").strip()
            raise SystemExit(f"smoke: FAILED: tesseract {' '.join(args)}: {err}")
        return done.stdout.decode("utf-8")

    said = run("--version")
    first = said.splitlines()[0]
    check(first == f"tesseract {version}", f"{first} (want tesseract {version})")
    found = [name for name in OPTIONAL_LIBRARIES if name in said]
    check(not found, f"tesseract is built without optional libraries (found {found})")
    langs = run("--list-langs", "--tessdata-dir", tessdata).split()
    check("eng" in langs and "hin" in langs, f"tesseract lists eng and hin in {tessdata}")

    def recognised(text, how):
        words = " ".join(text.split())
        for line in OCR_LINES:
            check(line in words, f"tesseract {how} reads {line!r} (got {words!r})")
        check(OCR_HINDI in words, f"tesseract {how} reads the Hindi word (got {words!r})")

    page = ocr_page(work)
    recognised(run(page, "stdout", "-l", "eng+hin", "--tessdata-dir", tessdata), "from a file")
    with open(page, "rb") as pgm:
        data = pgm.read()
    recognised(run("stdin", "stdout", "-l", "eng+hin", "--tessdata-dir", tessdata, stdin=data),
               "from stdin")
    if os.name == "nt":
        # Without --tessdata-dir or TESSDATA_PREFIX, Windows' tesseract looks beside itself.
        recognised(run(page, "stdout", "-l", "eng+hin"), "with its own tessdata")


def main():
    if len(sys.argv) != 6:
        raise SystemExit("usage: smoke.py <runtime dir> <fixtures dir> <work dir> <pandoc version> "
                         "<tesseract version>")
    root, fixtures, work, version, tesseract_version = sys.argv[1:6]
    interpreter(root)
    imports(root)
    docx_roundtrip(work)
    pandoc(root, work, version)
    xlsx_roundtrip(work)
    pptx_roundtrip(work)
    pdf_read(fixtures)
    numbers()
    tesseract(root, work, tesseract_version)
    print("smoke: all passed")


if __name__ == "__main__":
    main()
