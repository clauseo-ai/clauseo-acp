# Tests of src/pdftotext.py against PDFs written here byte by byte (text pages in Helvetica, scanned
# pages as images drawn with Pillow), with a fake tesseract that logs each page it is given. Run
# with an interpreter that has pypdfium2 and Pillow (and pypdf, for the encrypted file), from the
# repo root, as test_clauseo_pdf.py is:
#   <python> -m unittest discover -s crates/pdf/tests -p 'test_*.py'
# The test that OCRs a real scan needs Clauseo's tesseract: CLAUSEO_TEST_TESSERACT=<its folder>,
# which holds tesseract and tessdata/ (eng); without it, that test is skipped.
# On Windows the fake engine is the Rust one (src/bin/fake_tesseract.rs, never shipped), built by
# `cargo build -p clauseo-pdf --bins`: CLAUSEO_TEST_FAKE_TESSERACT=<its .exe>, and for the Job
# Object test CLAUSEO_TEST_PDFTOTEXT=<the pdftotext.exe launcher> too.
import ast
import ntpath
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
import zlib

SCRIPT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "src", "pdftotext.py"))

try:
    import pypdf
except ImportError:
    pypdf = None

# Runs the script's main with its engine, budget and caps from TEST_* variables (the shipped
# script finds tesseract only in its own runtime, with no variable to move it).
HARNESS = r"""
import importlib.util, os, sys
spec = importlib.util.spec_from_file_location("pdftotext", os.environ["TEST_SCRIPT"])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
engine = os.environ.get("TEST_ENGINE", "")
m.runtime = lambda: (engine, os.environ.get("TEST_TESSDATA", ""), "test-build")
for name in ("BUDGET", "MAX_OCR", "STALE"):
    if "TEST_" + name in os.environ:
        setattr(m, name, type(getattr(m, name))(os.environ["TEST_" + name]))
if "TEST_WORKERS" in os.environ:
    m.workers = lambda: int(os.environ["TEST_WORKERS"])
if "TEST_TEXT_FAIL" in os.environ:
    import pypdfium2
    opened = m.open_doc

    class Doc:
        def __init__(self, doc):
            self.doc = doc

        def __len__(self):
            return len(self.doc)

        def __getitem__(self, i):
            if i + 1 == int(os.environ["TEST_TEXT_FAIL"]):
                raise pypdfium2.PdfiumError("Failed to load page")
            return self.doc[i]

        def close(self):
            self.doc.close()
    m.open_doc = lambda *args: Doc(opened(*args))
if "TEST_RENDER_FAIL" in os.environ:
    render = m.pgm

    def pgm(doc, pg):
        if pg == int(os.environ["TEST_RENDER_FAIL"]):
            raise m.pdfium_error("Failed to fill bitmap rectangle")
        return render(doc, pg)
    import pypdfium2
    m.pdfium_error, m.pgm = pypdfium2.PdfiumError, pgm
code = m.main(sys.argv[1:])
if os.environ.get("TEST_NICE"):
    sys.stderr.write("nice %d\n" % os.getpriority(os.PRIO_PROCESS, 0))
sys.exit(code)
"""

# A fake tesseract: it checks the O2 rules (stdin, stdout, --tessdata-dir, no TESSDATA_PREFIX, a
# PGM on stdin), writes leptonica's chatter on stderr as the real one does, sleeps FAKE_SLEEP, logs
# "<langs> <width>" and prints words naming the width. src/bin/fake_tesseract.rs is the same.
FAKE = r"""
import os, sys, time
args = sys.argv[1:]
data = sys.stdin.buffer.read()
ok = (args[:2] == ["stdin", "stdout"] and args[2] == "-l" and args[4] == "--tessdata-dir"
      and len(args) == 6 and "TESSDATA_PREFIX" not in os.environ and data[:3] == b"P5\n"
      and os.environ.get("OMP_THREAD_LIMIT") == "1")
if not ok:
    sys.stderr.write("broken rules: %r\n" % (args,))
    sys.exit(3)
width = data.split()[1].decode()
if os.environ.get("FAKE_PIDS"):
    with open(os.environ["FAKE_PIDS"], "a") as pids:
        pids.write("%d\n" % os.getpid())
sys.stderr.write("Error in pixReadMemTiff: function not present\n"
                 "Error in pixReadMem: tiff: no pix returned\nEstimating resolution as 300\n")
if width == os.environ.get("FAKE_FAIL_WIDTH"):
    sys.stderr.write("Error: page broke\nError in pixaGenerateFont: pixa not made\n")
    sys.exit(1)
time.sleep(float(os.environ.get("FAKE_SLEEP", "0")))
if os.environ.get("FAKE_NICE"):
    with open(os.environ["FAKE_NICE"], "w") as nice:
        nice.write("%d" % os.getpriority(os.PRIO_PROCESS, 0))
hindi = os.environ.get("FAKE_HINDI") == "1" and "hin" in args[3]
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write("%s %s\n" % (args[3], width))
text = "\u0928\u094d\u092f\u093e\u092f\u093e\u0932\u092f " * 5 if hindi else "recognised words "
sys.stdout.write("%sw%s\n%s" % (text, width, "\f" if os.environ.get("FAKE_FF") else ""))
"""

# The fake engine is a "#!python" script, and Windows runs only an .exe; there the real-engine
# test (CLAUSEO_TEST_TESSERACT) covers the engine instead.
FAKE_EXE = os.environ.get("CLAUSEO_TEST_FAKE_TESSERACT", "")
fake_engine = unittest.skipIf(os.name == "nt" and not FAKE_EXE, "the fake engine is a Python "
                              "script; Windows runs only .exe: set CLAUSEO_TEST_FAKE_TESSERACT")


def ocr(pg, width, langs="eng+hin"):
    """A page OCR'd by the fake engine, as the output prints it."""
    return (f"[page {pg}: no text layer; recognised by OCR (tesseract, {langs}), errors are "
            f"possible]\nrecognised words w{width}\n")


def pdf_bytes(pages):
    """A PDF of `pages`: ("text", [lines]) in Helvetica, ("scan", image) a Pillow "L" image over
    the whole page, or ("blank", width) an empty page `width` pt wide (a scan to the fake engine,
    which tells pages apart by their width)."""
    objs = []

    def add(body):
        objs.append(body)
        return len(objs)

    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    parent = add(None)
    kids = []
    for page in pages:
        kind, size, res = page[0], (612, 792), b""
        if kind == "text":
            ops = b"BT /F1 12 Tf 14 TL 72 720 Td " + b"".join(
                b"(" + line.encode("latin-1") + b") Tj T* " for line in page[1]) + b"ET"
            res = b"/Font << /F1 %d 0 R >>" % font
        elif kind == "tounicode":
            # Helvetica whose ToUnicode map sends the capitals given (by default A, B and C to
            # ESC, BEL and C1's NEL) to other code points.
            chars = page[2] if len(page) > 2 else {"A": "001B", "B": "0007", "C": "0085"}
            pairs = b" ".join(b"<%02X> <%s>" % (ord(k), v.encode()) for k, v in chars.items())
            cmap = (b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName "
                    b"/T def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange "
                    b"%d beginbfchar %s endbfchar 1 beginbfrange "
                    b"<61> <7A> <0061> endbfrange endcmap CMapName currentdict /CMap "
                    b"defineresource pop end end" % (len(chars), pairs))
            uni = add(b"<< /Length %d >>\nstream\n%s\nendstream" % (len(cmap), cmap))
            tfont = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode %d 0 R >>"
                        % uni)
            ops = b"BT /F1 12 Tf 72 720 Td (" + page[1].encode() + b") Tj ET"
            res = b"/Font << /F1 %d 0 R >>" % tfont
        elif kind == "columns":
            ops = b"".join(b"BT /F1 12 Tf %d %d Td (%s) Tj ET " % (x, y, t.encode())
                           for x, y, t in page[1])
            res = b"/Font << /F1 %d 0 R >>" % font
        elif kind == "scan":
            image = page[1]
            raw = zlib.compress(image.tobytes())
            img = add(b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace "
                      b"/DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\n"
                      b"stream\n%s\nendstream" % (image.width, image.height, len(raw), raw))
            size = (image.width * 72 // 150, image.height * 72 // 150)
            ops = b"q %d 0 0 %d 0 0 cm /Im0 Do Q" % size
            res = b"/XObject << /Im0 %d 0 R >>" % img
        else:
            size, ops = (page[1], page[2] if len(page) > 2 else 300), b""
        content = add(b"<< /Length %d >>\nstream\n%s\nendstream" % (len(ops), ops))
        kids.append(add(b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] /Contents %d 0 R "
                        b"/Resources << %s >> >>" % (parent, size[0], size[1], content, res)))
    objs[parent - 1] = b"<< /Type /Pages /Kids [%s] /Count %d >>" % (
        b" ".join(b"%d 0 R" % k for k in kids), len(kids))
    catalog = add(b"<< /Type /Catalog /Pages %d 0 R >>" % parent)
    out, offsets = bytearray(b"%PDF-1.4\n"), []
    for i, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n%s\nendobj\n" % (i, body)
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    out += b"".join(b"%010d 00000 n \n" % o for o in offsets)
    out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1, catalog, xref)
    return bytes(out)


def many_pages(count):
    """A PDF of `count` empty pages that share one page tree entry's size: small to write."""
    objs = [b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /MediaBox [0 0 200 300] /Count %d /Kids [%s] >>" % (
                count, b" ".join(b"%d 0 R" % (3 + i) for i in range(count)))]
    objs += [b"<< /Type /Page /Parent 2 0 R >>"] * count
    out, offsets = bytearray(b"%PDF-1.4\n"), []
    for i, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n%s\nendobj\n" % (i, body)
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    out += b"".join(b"%010d 00000 n \n" % o for o in offsets)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
    return bytes(out)


def scan_image(lines):
    from PIL import Image, ImageDraw, ImageFont

    image = Image.new("L", (1275, 600), 255)
    draw = ImageDraw.Draw(image)
    font = ImageFont.load_default(size=36)
    for i, line in enumerate(lines):
        draw.text((80, 80 + 60 * i), line, fill=0, font=font)
    return image


def script():
    spec = importlib.util.spec_from_file_location("pdftotext_module", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PdfToText(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = os.path.realpath(self.temp.name)
        self.matter = os.path.join(self.dir, "matter")
        self.ocr = os.path.join(self.dir, "notes", "ocr")
        os.makedirs(self.matter)
        self.log = os.path.join(self.dir, "engine.log")
        self.fake = os.path.join(self.dir, "tesseract")
        with open(self.fake, "w") as file:
            file.write(f"#!{sys.executable}\n{FAKE}")
        os.chmod(self.fake, 0o755)
        if os.name == "nt" and FAKE_EXE:
            self.fake = FAKE_EXE
        self.tessdata = os.path.join(self.dir, "tessdata")
        os.makedirs(self.tessdata)

    def tearDown(self):
        self.temp.cleanup()

    def pdf(self, pages, name="doc.pdf"):
        path = os.path.join(self.matter, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as file:
            file.write(pdf_bytes(pages))
        return path

    def env(self, **extra):
        env = dict(os.environ, TEST_SCRIPT=SCRIPT, TEST_ENGINE=self.fake, FAKE_LOG=self.log,
                   TEST_TESSDATA=self.tessdata, CLAUSEO_OCR_DIR=self.ocr,
                   TESSDATA_PREFIX=self.dir)
        env.update({k: str(v) for k, v in extra.items()})
        return {k: v for k, v in env.items() if v != "None"}

    def start(self, *args, **extra):
        return subprocess.Popen(
            [sys.executable, "-c", HARNESS, "pdftotext", *args], cwd=self.matter,
            env=self.env(**extra), stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def run_tool(self, *args, **extra):
        proc = self.start(*args, **extra)
        out, err = proc.communicate(timeout=120)
        return proc.returncode, out.decode("utf-8"), err.decode("utf-8")

    def calls(self):
        try:
            with open(self.log) as file:
                return file.read().split("\n")[:-1]
        except FileNotFoundError:
            return []

    def mirror(self, rel):
        with open(os.path.join(self.ocr, rel + ".txt"), encoding="utf-8") as file:
            return file.read()

    def test_a_text_layer_pdf_prints_its_text_with_a_form_feed_after_each_page(self):
        self.pdf([("text", ["The plaintiff filed the suit on 3 March 2021.", "Second line."]),
                  ("text", ["Page two holds the reply of the defendant here."])])
        code, out, err = self.run_tool("doc.pdf", "-")
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "The plaintiff filed the suit on 3 March 2021.\nSecond line.\f"
                         "Page two holds the reply of the defendant here.\f")
        self.assertEqual(self.calls(), [])
        self.assertEqual(err, "")
        self.assertFalse(os.path.exists(os.path.join(self.ocr, "doc.pdf.txt")))

    @fake_engine
    def test_a_mixed_pdf_ocrs_only_its_scanned_page_and_says_so(self):
        self.pdf([("text", ["A typed first page with enough characters in it."]),
                  ("blank", 200), ("text", ["A typed third page with enough characters too."])])
        code, out, err = self.run_tool("doc.pdf", "-")
        self.assertEqual(code, 0, err)
        pages = out.split("\f")
        self.assertEqual(pages[1], ocr(2, 834))
        self.assertIn("typed first", pages[0])
        self.assertIn("typed third", pages[2])
        self.assertEqual(self.calls(), ["eng+hin 834"])
        self.assertIn("page 2 had no text layer and was recognised by OCR (tesseract, eng+hin); "
                      "recognition errors are possible", err)
        mirror = self.mirror("doc.pdf")
        self.assertTrue(mirror.startswith("# OCR of doc.pdf, sha256 "), mirror)
        self.assertIn("pages recognised 2 of 3", mirror)
        self.assertIn("typed third", mirror)

    def test_page_selection_and_output_to_a_file_stdout_or_beside_the_pdf(self):
        self.pdf([("text", [f"This is page number {n} of the typed bundle."]) for n in (1, 2, 3)])
        code, out, _ = self.run_tool("-f", "2", "-l", "2", "doc.pdf", "-")
        self.assertEqual((code, out), (0, "This is page number 2 of the typed bundle.\f"))
        code, out, _ = self.run_tool("-f", "3", "-nopgbrk", "doc.pdf", "-")
        self.assertEqual(out, "This is page number 3 of the typed bundle.")
        self.assertEqual(self.run_tool("doc.pdf", "out.txt")[:2], (0, ""))
        with open(os.path.join(self.matter, "out.txt"), encoding="utf-8") as file:
            self.assertEqual(file.read().count("\f"), 3)
        self.assertEqual(self.run_tool("doc.pdf")[0], 0)
        self.assertTrue(os.path.exists(os.path.join(self.matter, "doc.txt")))
        code, _, err = self.run_tool("-f", "3", "-l", "2", "doc.pdf", "-")
        self.assertEqual(code, 99)
        self.assertIn("Wrong page range", err)
        self.assertEqual(self.run_tool("-enc", "Latin1", "doc.pdf", "-")[0], 99)
        self.assertEqual(self.run_tool("-enc", "UTF-8", "-q", "doc.pdf", "-")[0], 0)
        self.assertEqual(self.run_tool("-bogus", "doc.pdf")[0], 99)
        code, _, err = self.run_tool("-v")
        self.assertEqual(code, 0)
        self.assertTrue(err.startswith("pdftotext version clauseo-1"), err)
        code, _, err = self.run_tool("missing.pdf", "-")
        self.assertEqual(code, 1)
        self.assertIn("Couldn't open file 'missing.pdf'", err)

    def test_layout_keeps_columns_apart(self):
        self.pdf([("columns", [(72, 700, "Left column words"), (400, 700, "Right column"),
                               (72, 680, "Second left line")])])
        code, out, _ = self.run_tool("-layout", "doc.pdf", "-")
        self.assertEqual(code, 0)
        first = out.split("\n")[0]
        self.assertRegex(first, r"^ *Left column words {10,}Right column$")
        self.assertIn("Second left line", out.split("\n")[1])

    @fake_engine
    def test_a_short_budget_prints_what_it_has_and_a_rerun_finishes_without_redoing_pages(self):
        self.pdf([("blank", 200 + 10 * n) for n in range(5)], name="Bundle.pdf")
        code, out, err = self.run_tool("Bundle.pdf", "-", TEST_BUDGET=1, FAKE_SLEEP=0.4,
                                       TEST_WORKERS=1)
        self.assertEqual(code, 0, err)
        first = self.calls()
        self.assertTrue(1 <= len(first) < 5, first)
        self.assertTrue(out.startswith("[Bundle.pdf: 0 of 5 pages read from the text layer, "), out)
        marker = out.split("\f")[-1]
        self.assertRegex(marker, r"^\[pages? [0-9, -]+ of Bundle\.pdf: not yet recognised; OCR "
                         r"stops after 1 seconds per run\. Run the same command again to "
                         r"continue, or use -f/-l for the pages you need\.\]\n$")
        self.assertIn(marker.strip(), err)
        self.assertIn("not yet read", self.mirror("Bundle.pdf"))
        code, out, err = self.run_tool("Bundle.pdf", "-")
        self.assertEqual(code, 0, err)
        widths = [call.split()[1] for call in self.calls()]
        self.assertEqual(len(widths), 5)
        self.assertEqual(len(set(widths)), 5)
        self.assertNotIn("not yet", out)
        self.assertEqual(out.count("recognised words"), 5)
        self.assertNotIn("not yet read", self.mirror("Bundle.pdf"))

    @fake_engine
    def test_a_cache_hit_runs_no_tesseract(self):
        self.pdf([("blank", 200), ("blank", 210)])
        first = self.run_tool("doc.pdf", "-")
        self.assertEqual(len(self.calls()), 2)
        second = self.run_tool("doc.pdf", "-")
        self.assertEqual(len(self.calls()), 2)
        self.assertEqual(first[1], second[1])
        self.assertIn("pages 1-2 had no text layer", second[2])

    @fake_engine
    def test_without_a_cache_folder_it_still_ocrs_and_keeps_nothing(self):
        self.pdf([("blank", 200)])
        code, out, _ = self.run_tool("doc.pdf", "-", CLAUSEO_OCR_DIR=None)
        self.assertEqual((code, out), (0, ocr(1, 834) + "\f"))
        self.assertFalse(os.path.exists(self.ocr))

    @fake_engine
    def test_a_changed_byte_misses_and_a_rename_reuses_by_hash(self):
        path = self.pdf([("blank", 200), ("blank", 210)])
        self.run_tool("doc.pdf", "-")
        renamed = os.path.join(self.matter, "Pleadings", "Reply.pdf")
        os.makedirs(os.path.dirname(renamed))
        os.rename(path, renamed)
        code, out, _ = self.run_tool(os.path.join("Pleadings", "Reply.pdf"), "-")
        self.assertEqual(code, 0)
        self.assertEqual(len(self.calls()), 2)
        self.assertIn("pages recognised 1-2 of 2", self.mirror(os.path.join("Pleadings",
                                                                            "Reply.pdf")))
        with open(renamed, "ab") as file:
            file.write(b"%")
        self.run_tool(renamed, "-")
        self.assertEqual(len(self.calls()), 4)

    @fake_engine
    def test_two_processes_at_once_do_one_ocr(self):
        self.pdf([("blank", 200 + 10 * n) for n in range(4)])
        procs = [self.start("doc.pdf", "-", FAKE_SLEEP=0.3, TEST_WORKERS=2) for _ in range(2)]
        outs = [proc.communicate(timeout=120) for proc in procs]
        self.assertEqual([p.returncode for p in procs], [0, 0], outs)
        self.assertEqual(len(self.calls()), 4, self.calls())
        self.assertEqual(outs[0][0], outs[1][0])
        self.assertEqual(outs[0][0].decode().count("recognised words"), 4)

    @fake_engine
    def test_a_dead_holders_lock_is_taken_over(self):
        self.pdf([("blank", 200), ("blank", 210)])
        self.run_tool("-l", "1", "doc.pdf", "-")
        [folder] = os.listdir(os.path.join(self.ocr, ".by-hash"))
        lock = os.path.join(self.ocr, ".by-hash", folder, "lock")
        with open(lock, "w") as file:
            file.write("{}")
        code, out, _ = self.run_tool("doc.pdf", "-", TEST_BUDGET=2)
        self.assertIn("[page 2 of doc.pdf: not yet recognised", out)
        self.assertEqual(len(self.calls()), 1)
        os.utime(lock, (time.time() - 60, time.time() - 60))
        code, out, _ = self.run_tool("doc.pdf", "-")
        self.assertEqual((code, out), (0, ocr(1, 834) + "\f" + ocr(2, 876, "eng") + "\f"))
        self.assertFalse(os.path.exists(lock))

    @fake_engine
    def test_the_language_probe_keeps_hindi_only_when_the_first_scan_has_it(self):
        self.pdf([("blank", 200 + 10 * n) for n in range(3)])
        self.run_tool("doc.pdf", "-", TEST_WORKERS=1)
        self.assertEqual([c.split()[0] for c in self.calls()], ["eng+hin", "eng", "eng"])
        os.remove(self.log)
        self.pdf([("blank", 300 + 10 * n) for n in range(3)], name="hindi.pdf")
        code, out, err = self.run_tool("hindi.pdf", "-", FAKE_HINDI=1, TEST_WORKERS=1)
        self.assertEqual([c.split()[0] for c in self.calls()], ["eng+hin"] * 3)
        self.assertIn("\u0928\u094d\u092f\u093e\u092f", out)
        self.assertIn("(tesseract, eng+hin)", err)

    def test_an_encrypted_or_damaged_file_is_recorded_and_not_retried(self):
        with open(os.path.join(self.matter, "bad.pdf"), "wb") as file:
            file.write(b"%PDF-1.4\nnot really a pdf at all\n")
        code, _, err = self.run_tool("bad.pdf", "-")
        self.assertEqual(code, 1)
        self.assertIn("pdftotext: bad.pdf is damaged or not a PDF; its text cannot be read. "
                      "Recorded; Clauseo will not try this file again until it changes.", err)
        [folder] = os.listdir(os.path.join(self.ocr, ".by-hash"))
        with open(os.path.join(self.ocr, ".by-hash", folder, "failed.json")) as file:
            self.assertEqual(json.load(file)["reason"], "damaged")
        code, _, err = self.run_tool("bad.pdf", "-")
        self.assertEqual(code, 1)
        self.assertRegex(err, r"Recorded on \d{4}-\d\d-\d\dT")
        code, _, err = self.run_tool("bad.pdf", "-", CLAUSEO_OCR_DIR=None)
        self.assertNotIn("Recorded", err)
        if pypdf is None:
            self.skipTest("pypdf is not installed")
        writer = pypdf.PdfWriter()
        writer.add_blank_page(200, 200)
        writer.encrypt("secret")
        with open(os.path.join(self.matter, "locked.pdf"), "wb") as file:
            writer.write(file)
        code, _, err = self.run_tool("locked.pdf", "-")
        self.assertEqual(code, 1)
        self.assertIn("locked.pdf is encrypted (it needs a password)", err)
        self.assertEqual(self.calls(), [])

    @fake_engine
    def test_the_page_cap_names_the_pages_past_it(self):
        self.pdf([("blank", 200 + 10 * n) for n in range(4)])
        code, out, _ = self.run_tool("doc.pdf", "-", TEST_MAX_OCR=2)
        self.assertEqual(len(self.calls()), 2)
        self.assertTrue(out.endswith(
            "[pages 3-4 of doc.pdf: not recognised; OCR covers at most 2 pages per run. Use "
            "-f/-l to choose the pages, at most 2 at a time.]\n"), out)
        code, out, _ = self.run_tool("-f", "3", "doc.pdf", "-", TEST_MAX_OCR=2)
        self.assertEqual(len(self.calls()), 4)

    def test_without_tesseract_the_scanned_pages_are_named(self):
        self.pdf([("blank", 200)])
        code, out, _ = self.run_tool("doc.pdf", "-", TEST_ENGINE=os.path.join(self.dir, "none"))
        self.assertEqual(code, 0)
        self.assertIn("[page 1 of doc.pdf: no text layer, and OCR is not available", out)

    @fake_engine
    def test_non_ascii_names_print_utf8_whatever_the_console(self):
        name = os.path.join("दस्तावेज़", "Écrit 1.pdf")
        self.pdf([("blank", 200)], name=name)
        code, out, err = self.run_tool(name, "-", PYTHONIOENCODING="ascii")
        self.assertEqual(code, 0, err)
        self.assertEqual(out, ocr(1, 834) + "\f")
        self.assertIn("pages recognised 1 of 1", self.mirror(name))

    def test_windows_paths_mirror_under_the_matter_and_nothing_outside_it(self):
        mirror_path = script().mirror_path
        root = "C:\\Data\\agent-state\\m1\\notes\\ocr"
        cwd = "C:\\Matters\\Sharma v. Union"
        got = mirror_path(root, "Pleadings\\Reply.pdf", cwd, path=ntpath)
        self.assertEqual(got, (root + "\\Pleadings\\Reply.pdf.txt", "Pleadings\\Reply.pdf"))
        got = mirror_path(root, cwd + "\\Annex\\दस्तावेज़.pdf", cwd, path=ntpath)
        self.assertEqual(got[1], "Annex\\दस्तावेज़.pdf")
        self.assertEqual(mirror_path(root, "D:\\Other\\a.pdf", cwd, path=ntpath), (None, None))
        self.assertEqual(mirror_path(root, "..\\a.pdf", cwd, path=ntpath), (None, None))
        self.assertEqual(mirror_path(root, "/tmp/a.pdf", "/m", path=os.path), (None, None))

    def test_the_script_parses(self):
        with open(SCRIPT, encoding="utf-8") as file:
            ast.parse(file.read())

    @fake_engine
    def test_a_slow_engine_is_stopped_inside_the_budget(self):
        self.pdf([("blank", 200), ("blank", 210)])
        began = time.monotonic()
        code, out, err = self.run_tool("doc.pdf", "-", TEST_BUDGET=2, FAKE_SLEEP=60)
        self.assertLess(time.monotonic() - began, 10)
        self.assertEqual(code, 0, err)
        self.assertIn("[pages 1-2 of doc.pdf: not yet recognised", out)
        self.assertEqual(self.calls(), [])

    @fake_engine
    def test_a_failed_page_is_named_and_the_others_are_kept(self):
        self.pdf([("blank", 200), ("blank", 210), ("blank", 220)])
        code, out, err = self.run_tool("doc.pdf", "-", FAKE_FAIL_WIDTH=876, TEST_WORKERS=1)
        self.assertEqual(code, 0, err)
        self.assertEqual(out.count("recognised words"), 2)
        self.assertIn("[page 2 of doc.pdf: OCR failed (tesseract: Error: page broke); run the "
                      "same command again to retry, or check the page image.]", out)
        self.assertNotIn("not yet recognised", out)
        code, out, _ = self.run_tool("doc.pdf", "-")
        self.assertEqual(out.count("recognised words"), 3)
        self.assertEqual(len(self.calls()), 3)

    @unittest.skipUnless(hasattr(os, "kill") and os.name != "nt", "Unix signals")
    def test_sigterm_leaves_no_tesseract_behind_in_the_probe_or_the_workers(self):
        import signal

        pids = os.path.join(self.dir, "pids")
        self.pdf([("blank", 200 + 10 * n) for n in range(3)])
        # First the language probe (page 1 of a fresh document, alone), then two workers (pages
        # 2-3, once page 1 has decided the languages).
        for args, want in ((("-l", "1"), 1), (("-f", "2"), 2)):
            proc = self.start(*args, "doc.pdf", "-", FAKE_SLEEP=60, FAKE_PIDS=pids,
                              TEST_WORKERS=2)
            started = time.monotonic()
            while time.monotonic() - started < 30:
                if os.path.exists(pids):
                    with open(pids) as file:
                        if len(file.read().split()) >= want:
                            break
                time.sleep(0.05)
            with open(pids) as file:
                children = [int(p) for p in file.read().split()]
            self.assertEqual(len(children), want)
            proc.send_signal(signal.SIGTERM)
            proc.communicate(timeout=30)
            self.assertEqual(proc.returncode, 130)
            for pid in children:
                for _ in range(100):
                    try:
                        os.kill(pid, 0)
                    except ProcessLookupError:
                        break
                    time.sleep(0.05)
                else:
                    self.fail(f"tesseract {pid} outlived pdftotext")
            os.remove(pids)
            if want == 1:
                self.assertEqual(self.run_tool("-l", "1", "doc.pdf", "-")[0], 0)

    def test_runtime_finds_tesseract_beside_its_own_interpreter(self):
        runtime = script().runtime
        root = os.path.join(self.dir, "clauseo-python-3.13.18-c4")
        with self.subTest("macOS"):
            os.makedirs(os.path.join(root, "bin"))
            with open(os.path.join(root, "clauseo-python.json"), "w") as file:
                json.dump({"schema": 1, "archive": "clauseo-python-c4-mac.tar.gz"}, file)
            got = runtime(os.path.join(root, "bin", "python3"), windows=False)
            self.assertEqual(got, (os.path.join(root, "bin", "tesseract"),
                                   os.path.join(root, "bin", "tessdata"),
                                   "clauseo-python-c4-mac.tar.gz"))
        with self.subTest("Windows"):
            got = runtime(os.path.join(root, "python.exe"), windows=True)
            self.assertEqual(got, (os.path.join(root, "clauseo-bin", "tesseract.exe"),
                                   os.path.join(root, "clauseo-bin", "tessdata"),
                                   "clauseo-python-c4-mac.tar.gz"))
        with self.subTest("no receipt"):
            got = runtime(os.path.join(self.dir, "bin", "python3"), windows=False)
            self.assertEqual(got[2], "?")

    @unittest.skipUnless(os.name != "nt", "a symlinked interpreter")
    def test_the_shipped_script_runs_the_tesseract_in_its_runtime(self):
        # No harness: the script as shipped, run by an interpreter linked into a runtime-shaped
        # folder whose bin/ holds the fake tesseract and tessdata/.
        root = os.path.join(self.dir, "rt")
        os.makedirs(os.path.join(root, "bin", "tessdata"))
        os.symlink(sys.executable, os.path.join(root, "bin", "python3"))
        os.symlink(self.fake, os.path.join(root, "bin", "tesseract"))
        with open(os.path.join(root, "clauseo-python.json"), "w") as file:
            json.dump({"schema": 1, "archive": "rt-test"}, file)
        self.pdf([("blank", 200)])
        done = subprocess.run([os.path.join(root, "bin", "python3"), SCRIPT, "pdftotext",
                               "doc.pdf", "-"], cwd=self.matter, env=self.env(),
                              capture_output=True, timeout=120)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.decode(), ocr(1, 834) + "\f")
        self.assertEqual(self.calls(), ["eng+hin 834"])
        [folder] = os.listdir(os.path.join(self.ocr, ".by-hash"))
        self.assertTrue(folder.endswith("-" + __import__("hashlib").sha256(
            b"rt-test|tesseract|300|1").hexdigest()[:8]), folder)

    def test_a_page_too_big_renders_under_the_pixel_cap(self):
        module = script()
        import pypdfium2 as pdfium

        path = self.pdf([("blank", 14_000)])
        doc = pdfium.PdfDocument(path)
        image = module.pgm(doc, 1)
        doc.close()
        width, height = (int(v) for v in bytes(image[:40]).split()[1:3])
        self.assertLessEqual(width * height, module.MAX_PIXELS)
        self.assertEqual(len(image), len(b"P5\n%d %d\n255\n" % (width, height)) + width * height)

    def test_a_page_too_long_renders_under_the_side_cap(self):
        module = script()
        import pypdfium2 as pdfium

        doc = pdfium.PdfDocument(self.pdf([("blank", 100_000)]))
        image = module.pgm(doc, 1)
        doc.close()
        width, height = (int(v) for v in bytes(image[:40]).split()[1:3])
        self.assertLessEqual(max(width, height), 20_000)

    @fake_engine
    def test_a_page_pdfium_cannot_render_is_named_and_the_others_are_kept(self):
        self.pdf([("text", ["A typed first page with enough characters in it."]),
                  ("blank", 200), ("blank", 210)])
        code, out, err = self.run_tool("doc.pdf", "-", TEST_RENDER_FAIL=2, TEST_WORKERS=1)
        self.assertEqual(code, 0, err)
        self.assertIn("typed first", out)
        self.assertIn(ocr(3, 876), out)
        self.assertIn("[page 2 of doc.pdf: OCR failed (PDFium could not render it: PdfiumError: "
                      "Failed to fill bitmap rectangle); run the same command again", out)

    def test_pages_past_the_budget_are_named_in_time_however_many(self):
        # 150,000 pages, none read (no budget): the output once took time in the square of that.
        with open(os.path.join(self.matter, "big.pdf"), "wb") as file:
            file.write(many_pages(150_000))
        began = time.monotonic()
        code, out, err = self.run_tool("big.pdf", "-", TEST_BUDGET=0)
        self.assertLess(time.monotonic() - began, 30)
        self.assertEqual(code, 0, err)
        self.assertIn("[pages 1-150000 of big.pdf: not read;", out)

    def test_control_characters_never_reach_the_output(self):
        self.pdf([("tounicode", "abcdefghijklmnopqrstuvwxyzABCabc")])
        code, out, err = self.run_tool("doc.pdf", "-")
        self.assertEqual(code, 0, err)
        self.assertEqual(out, "abcdefghijklmnopqrstuvwxyzabc\f")
        out = self.run_tool("-layout", "doc.pdf", "-")[1]
        self.assertEqual(out.strip(" "), "abcdefghijklmnopqrstuvwxyzabc\n\f")

    @fake_engine
    def test_ocr_text_carries_its_marker_in_the_output_file_even_with_q(self):
        self.pdf([("blank", 200)])
        self.assertEqual(self.run_tool("-q", "doc.pdf")[:3], (0, "", ""))
        with open(os.path.join(self.matter, "doc.txt"), encoding="utf-8") as file:
            self.assertEqual(file.read(), ocr(1, 834) + "\f")
        self.assertTrue(self.mirror("doc.pdf").split("\n", 1)[1].startswith(ocr(1, 834)))

    @fake_engine
    def test_without_a_cache_folder_a_short_budget_says_a_rerun_starts_over(self):
        self.pdf([("blank", 200 + 10 * n) for n in range(5)])
        code, out, err = self.run_tool("doc.pdf", "-", TEST_BUDGET=1, FAKE_SLEEP=0.4,
                                       TEST_WORKERS=1, CLAUSEO_OCR_DIR=None)
        self.assertEqual(code, 0, err)
        self.assertNotIn("again to continue", out)
        self.assertRegex(out, r"not yet recognised; OCR stops after 1 seconds per run\. A rerun "
                         r"starts over \(this run keeps no OCR cache\); use -f/-l for the pages "
                         r"you need\.\]\n$")

    @fake_engine
    def test_cached_languages_other_than_eng_or_eng_hin_are_not_used(self):
        data = pdf_bytes([("blank", 200), ("blank", 210)])
        self.pdf([("blank", 200), ("blank", 210)])
        folder = os.path.join(self.ocr, ".by-hash", "%s-%s" % (
            __import__("hashlib").sha256(data).hexdigest(),
            __import__("hashlib").sha256(b"test-build|tesseract|300|1").hexdigest()[:8]))
        os.makedirs(folder)
        with open(os.path.join(folder, "meta.json"), "w") as file:
            json.dump({"langs": "fra"}, file)
        with open(os.path.join(folder, "p0002.json"), "w") as file:
            json.dump({"kind": "ocr", "langs": "eng --psm 0", "text": "planted\n"}, file)
        code, out, err = self.run_tool("doc.pdf", "-", TEST_WORKERS=1)
        self.assertEqual(code, 0, err)
        # The probe asks for eng+hin and finds no Hindi, so eng.
        self.assertEqual(self.calls(), ["eng+hin 834", "eng 876"])
        self.assertNotIn("planted", out)

    @unittest.skipIf(not hasattr(os, "getpriority") or os.getpriority(os.PRIO_PROCESS, 0) >= 10,
                     "Unix, from a priority above 10")
    def test_the_whole_run_and_its_tesseract_run_at_low_priority(self):
        nice = os.path.join(self.dir, "nice")
        self.pdf([("text", ["A typed first page with enough characters in it."]), ("blank", 200)])
        code, _, err = self.run_tool("doc.pdf", "-", TEST_NICE=1, FAKE_NICE=nice)
        self.assertEqual(code, 0, err)
        self.assertIn("nice 10\n", err)
        with open(nice) as file:
            self.assertEqual(file.read(), "10")

    @fake_engine
    def test_tesseract_chatter_stays_out_of_stderr_and_out_of_a_failure(self):
        self.pdf([("blank", 200), ("blank", 210)])
        code, out, err = self.run_tool("doc.pdf", "-", FAKE_FAIL_WIDTH=876, TEST_WORKERS=1)
        self.assertEqual(code, 0, err)
        self.assertIn("OCR: page 2 failed: tesseract: Error: page broke\n", err)
        for text in (out, err):
            self.assertNotIn("Error in pix", text)
            self.assertNotIn("Estimating", text)

    def test_a_form_feed_in_a_pages_text_becomes_a_line_break(self):
        self.pdf([("tounicode", "abcdefghijklmnopqrstuvwxyzAabc", {"A": "000C"}),
                  ("blank", 200)])
        code, out, err = self.run_tool("doc.pdf", "-", TEST_ENGINE=os.path.join(self.dir, "x"))
        self.assertEqual(code, 0, err)
        self.assertEqual(out.split("\f")[0].split("\n", 1)[1], "abcdefghijklmnopqrstuvwxyz\nabc")
        self.assertEqual(out.count("\f"), 2)

    @fake_engine
    def test_tesseracts_trailing_form_feed_is_not_a_page_break(self):
        self.pdf([("blank", 200), ("blank", 210)])
        code, out, err = self.run_tool("doc.pdf", "-", FAKE_FF=1)
        self.assertEqual((code, out), (0, ocr(1, 834) + "\f" + ocr(2, 876, "eng") + "\f"), err)
        mirror = self.mirror("doc.pdf")
        self.assertEqual(mirror.split("\n", 1)[1], ocr(1, 834) + "\f" + ocr(2, 876, "eng") + "\f")

    def test_a_page_pdfium_cannot_load_is_named_and_the_others_are_kept(self):
        self.pdf([("text", [f"This is page number {n} of the typed bundle."]) for n in (1, 2, 3)])
        code, out, err = self.run_tool("doc.pdf", "-", TEST_TEXT_FAIL=2)
        self.assertEqual(code, 0, err)
        self.assertIn("page number 1", out)
        self.assertIn("page number 3", out)
        self.assertIn("[page 2 of doc.pdf: not read (PDFium: PdfiumError: Failed to load page); "
                      "run the same command again", out)
        self.assertIn("1 failed", out.split("\n")[0])

    def test_partial_output_says_so_on_its_first_line(self):
        self.pdf([("text", [f"This is page number {n} of the typed bundle."]) for n in (1, 2)]
                 + [("blank", 200)])
        code, out, err = self.run_tool("doc.pdf", "-", TEST_ENGINE=os.path.join(self.dir, "x"))
        self.assertEqual(code, 0, err)
        self.assertTrue(out.startswith(
            "[doc.pdf: 2 of 3 pages read from the text layer, 0 by OCR; 1 not read, 0 failed. "
            "The notes at the end name them.]\nThis is page number 1"), out)
        self.assertTrue(self.run_tool("doc.pdf", "-", TEST_MAX_OCR=0)[1].startswith(
            "[doc.pdf: 2 of 3 pages read from the text layer, 0 by OCR; 1 not read"))

    @unittest.skipUnless(os.environ.get("CLAUSEO_TEST_TESSERACT"), "CLAUSEO_TEST_TESSERACT unset")
    def test_a_scanned_page_yields_its_words_with_the_real_engine(self):
        folder = os.environ["CLAUSEO_TEST_TESSERACT"]
        engine = os.path.join(folder, "tesseract.exe" if os.name == "nt" else "tesseract")
        lines = ["IN THE HIGH COURT OF DELHI", "The petition is allowed with costs."]
        self.pdf([("scan", scan_image(lines))])
        code, out, err = self.run_tool(
            "doc.pdf", "-", TEST_ENGINE=engine,
            TEST_TESSDATA=os.path.join(folder, "tessdata"))
        self.assertEqual(code, 0, err)
        # Exactly: the page's marker, its two lines (blank lines aside), and one \f.
        marker, words = out.split("\n", 1)
        self.assertEqual(marker, "[page 1: no text layer; recognised by OCR (tesseract, "
                         "eng+hin), errors are possible]")
        self.assertTrue(words.endswith("\f") and words.count("\f") == 1, repr(words))
        self.assertEqual([ln.strip() for ln in words[:-1].split("\n") if ln.strip()], lines)


def wait_for_pids(path, want, seconds=30):
    started = time.monotonic()
    while time.monotonic() - started < seconds:
        if os.path.exists(path):
            with open(path) as file:
                pids = [int(p) for p in file.read().split()]
            if len(pids) >= want:
                return pids
        time.sleep(0.05)
    raise AssertionError(f"no {want} tesseract started")


@unittest.skipUnless(os.name == "nt" and FAKE_EXE, "Windows, with CLAUSEO_TEST_FAKE_TESSERACT")
class WindowsEngine(unittest.TestCase):
    """Windows only, with the Rust fake: what the Unix signal tests check there, the Windows way."""

    setUp, tearDown, pdf, env, start = (PdfToText.setUp, PdfToText.tearDown, PdfToText.pdf,
                                        PdfToText.env, PdfToText.start)

    def kernel32(self):
        import ctypes

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.OpenProcess.restype = ctypes.c_void_p
        k32.GetPriorityClass.argtypes = k32.CloseHandle.argtypes = [ctypes.c_void_p]
        k32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        return k32

    def test_tesseract_runs_below_normal_priority(self):
        k32, pids = self.kernel32(), os.path.join(self.dir, "pids")
        self.pdf([("blank", 200)])
        proc = self.start("doc.pdf", "-", FAKE_SLEEP=3, FAKE_PIDS=pids)
        [pid] = wait_for_pids(pids, 1)
        handle = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        self.assertTrue(handle)
        try:
            self.assertEqual(k32.GetPriorityClass(handle), 0x4000)  # BELOW_NORMAL
        finally:
            k32.CloseHandle(handle)
        self.assertEqual(proc.communicate(timeout=60)[0].decode().count("recognised words"), 1)

    @unittest.skipUnless(os.environ.get("CLAUSEO_TEST_PDFTOTEXT"), "CLAUSEO_TEST_PDFTOTEXT unset")
    def test_killing_the_launcher_takes_python_and_tesseract_down(self):
        # A runtime-shaped folder: a venv of this interpreter (which has pypdfium2 in its own
        # site-packages, not --user: the launcher runs Python -I) with the receipt and the fake
        # engine as clauseo-bin\tesseract.exe, on PATH alone; then the launcher as an agent runs it.
        import shutil

        k32, pids = self.kernel32(), os.path.join(self.dir, "pids")
        venv = os.path.join(self.dir, "rt")
        subprocess.run([sys.executable, "-m", "venv", "--system-site-packages", "--without-pip",
                        venv], check=True, timeout=120)
        scripts = os.path.join(venv, "Scripts")
        with open(os.path.join(scripts, "clauseo-python.json"), "w") as file:
            json.dump({"schema": 1, "archive": "rt-test"}, file)
        os.makedirs(os.path.join(scripts, "clauseo-bin", "tessdata"))
        shutil.copy(FAKE_EXE, os.path.join(scripts, "clauseo-bin", "tesseract.exe"))
        self.pdf([("blank", 200)])
        env = self.env(FAKE_SLEEP=60, FAKE_PIDS=pids, PATH=scripts)
        proc = subprocess.Popen([os.environ["CLAUSEO_TEST_PDFTOTEXT"], "doc.pdf", "-"],
                                cwd=self.matter, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE)
        [pid] = wait_for_pids(pids, 1, 60)
        handle = k32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        self.assertTrue(handle, f"tesseract {pid} could not be opened while it ran")
        try:
            proc.kill()
            proc.communicate(timeout=30)
            self.assertEqual(k32.WaitForSingleObject(handle, 10_000), 0,
                             f"tesseract {pid} outlived pdftotext.exe")
        finally:
            k32.CloseHandle(handle)


if __name__ == "__main__":
    unittest.main()
