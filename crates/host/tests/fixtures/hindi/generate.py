#!/usr/bin/env python3
"""Generates the Hindi fixture matter in this folder (see README.md).

Run from anywhere:

    python3 generate.py /path/to/NotoSansDevanagari-Regular.ttf

Needs Python 3.11 with reportlab 4.4 and uharfbuzz (the shaping), pikepdf (the legacy-encoded
copy), PyMuPDF (the scan's rasterising), Pillow and python-docx. It writes the five papers next
to itself and nothing else. The docx ones need no font: `letter()` alone makes them. The planted fact (TENANT, AMOUNT, DATE below) is also in
`crates/host/tests/host/hindi/verdict.rs`; a host test fails when the two disagree.
"""

import io
import sys
import unicodedata
import zipfile
from pathlib import Path

import docx
import fitz
import pikepdf
from docx.oxml.ns import qn
from docx.shared import Pt
from PIL import Image
from reportlab import rl_config
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import Paragraph, SimpleDocTemplate

HERE = Path(__file__).resolve().parent

# The planted fact. It is in kirayanama.pdf (ASCII digits) and patra.docx (Devanagari digits)
# only; the legacy-encoded notice and the scanned affidavit never mention it.
TENANT = "सुरेखा भार्गव"
AMOUNT = "3,47,215"
AMOUNT_DEVANAGARI = "३,४७,२१५"
DATE = "17 मार्च 2024"
DATE_DEVANAGARI = "१७ मार्च २०२४"

LEASE = [
    "किरायानामा",
    f"यह किरायानामा {DATE} को लखनऊ में मकान मालिक श्री दीनदयाल मिश्र और किरायेदार "
    f"श्रीमती {TENANT} के बीच हुआ।",
    f"किरायेदार ने जमानत राशि ₹{AMOUNT} (तीन लाख सैंतालीस हज़ार दो सौ पंद्रह रुपये) नक़द जमा की।",
    "मासिक किराया ₹18,500 है, जो हर महीने की 5 तारीख़ तक देय होगा।",
    "यह क़रार ग्यारह महीने के लिए है और दोनों पक्षों की सहमति से बढ़ाया जा सकता है।",
]

NOTICE = [
    "कानूनी नोटिस",
    "मकान संख्या 42, गोमती नगर, लखनऊ में पानी की आपूर्ति पिछले तीन सप्ताह से बंद है।",
    "मकान मालिक से अनुरोध है कि पंद्रह दिन के भीतर मरम्मत कराएँ, अन्यथा उचित कार्यवाही की जाएगी।",
]

AFFIDAVIT = [
    "शपथपत्र",
    "मैं, दीनदयाल मिश्र, पुत्र स्वर्गीय रामनाथ मिश्र, शपथपूर्वक कहता हूँ कि मकान संख्या 42 "
    "मेरे नाम पर पंजीकृत है।",
    "मैंने मकान की छत की मरम्मत अपने ख़र्च पर करवाई है और उसके बिल मेरे पास सुरक्षित हैं।",
]

LETTER = [
    "सेवा में, श्री दीनदयाल मिश्र, गोमती नगर, लखनऊ",
    f"मेरी मुवक्किल श्रीमती {TENANT} ने दिनांक {DATE_DEVANAGARI} के किरायानामे के अधीन "
    f"₹{AMOUNT_DEVANAGARI} की जमानत राशि जमा की थी, जो क़रार समाप्त होने पर लौटाई जानी है।",
    # The nukta paragraph: ज़, क़, फ़ in NFC, which keeps them as base letter + U+093C.
    "ज़मीन के क़ब्ज़े के बारे में अंतिम फ़ैसला अदालत करेगी।",
    "भवदीय, अधिवक्ता",
]


# patra-edited.docx: the same letter with one paragraph added before the signature. It stands
# in for the edit a stetdoc change makes in the Word document (tests/host/word_round_trip.rs).
EDIT = "संशोधन: यह पत्र १८ मार्च २०२४ को संशोधित किया गया।"
LETTER_EDITED = LETTER[:-1] + [EDIT] + LETTER[-1:]


def nfc(lines):
    return [unicodedata.normalize("NFC", line) for line in lines]


def lease_style():
    return ParagraphStyle(
        "hindi", fontName="Deva", fontSize=14, leading=24, spaceAfter=10, shaping=1
    )


class Spoken(Paragraph):
    """A shaped paragraph inside a marked-content span whose ActualText is its text, so text
    extraction gives the logical Unicode text. The ToUnicode map alone cannot: a shaped glyph
    (a reordered i-matra, a conjunct, a reph) stands for letters in another order or for
    several letters at once."""

    def draw(self):
        actual = "FEFF" + self.actual.encode("utf-16-be").hex().upper()
        self.canv.addLiteral(f"/Span <</ActualText <{actual}>>> BDC")
        super().draw()
        self.canv.addLiteral("EMC")


def paragraph(line, actual_text):
    para = Spoken(line, lease_style()) if actual_text else Paragraph(line, lease_style())
    para.actual = line
    return para


def pdf(lines, actual_text=True):
    """A Unicode PDF: shaped glyphs from the embedded subset, a ToUnicode map, and each
    paragraph's text as ActualText."""
    out = io.BytesIO()
    doc = SimpleDocTemplate(out, pagesize=A4, title="", author="", creator="", producer="")
    doc.build([paragraph(line, actual_text) for line in nfc(lines)])
    return out.getvalue()


def legacy(lines):
    """The same kind of PDF with no ActualText and every ToUnicode map removed: the glyphs
    still draw, but text extraction gives the subset's one-byte codes (control characters
    and WinAnsi symbols), the way a PDF set in a legacy Hindi font does. No legacy font is
    used."""
    source = pikepdf.open(io.BytesIO(pdf(lines, actual_text=False)))
    for page in source.pages:
        for font in page.Resources.Font.values():
            if "/ToUnicode" in font:
                del font["/ToUnicode"]
    out = io.BytesIO()
    source.save(out, deterministic_id=True, compress_streams=True)
    return out.getvalue()


def scan(lines):
    """A page of the Unicode kind rasterised at 110 dpi to greyscale JPEG, in a PDF with no
    text layer."""
    page = fitz.open(stream=pdf(lines), filetype="pdf")[0]
    pixmap = page.get_pixmap(dpi=110, colorspace=fitz.csGRAY)
    image = Image.frombytes("L", (pixmap.width, pixmap.height), pixmap.samples)
    out = io.BytesIO()
    image.save(out, format="PDF", resolution=110.0, quality=60)
    # Pillow writes the time into the document info; drop it so a rerun gives the same bytes.
    scanned = pikepdf.open(io.BytesIO(out.getvalue()))
    del scanned.trailer["/Info"]
    fixed = io.BytesIO()
    scanned.save(fixed, deterministic_id=True)
    return fixed.getvalue()


def letter(lines):
    document = docx.Document()
    style = document.styles["Normal"]
    style.font.size = Pt(14)
    style.font.name = "Noto Sans Devanagari"
    style.element.rPr.rFonts.set(qn("w:cs"), "Noto Sans Devanagari")
    for line in nfc(lines):
        document.add_paragraph(line)
    document.core_properties.author = ""
    out = io.BytesIO()
    document.save(out)
    return fixed_zip(out.getvalue())


def fixed_zip(data):
    """The docx again with every entry's time fixed, so a rerun gives the same bytes."""
    source = zipfile.ZipFile(io.BytesIO(data))
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as target:
        for info in source.infolist():
            fixed = zipfile.ZipInfo(info.filename, date_time=(2024, 3, 17, 0, 0, 0))
            fixed.compress_type = zipfile.ZIP_DEFLATED
            target.writestr(fixed, source.read(info.filename))
    return out.getvalue()


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: generate.py /path/to/NotoSansDevanagari-Regular.ttf")
    rl_config.invariant = 1
    pdfmetrics.registerFont(TTFont("Deva", sys.argv[1], shapable=True))
    papers = {
        "kirayanama.pdf": pdf(LEASE),
        "notice-legacy.pdf": legacy(NOTICE),
        "affidavit-scan.pdf": scan(AFFIDAVIT),
        "patra.docx": letter(LETTER),
        "patra-edited.docx": letter(LETTER_EDITED),
    }
    for name, data in papers.items():
        (HERE / name).write_bytes(data)
        print(f"{name}: {len(data)} bytes")


if __name__ == "__main__":
    main()
