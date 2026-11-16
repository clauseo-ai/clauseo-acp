# Tests of src/clauseo_pdf.py against PDFs made here with pypdfium2 (and pypdf, for the encrypted
# one). Run with an interpreter that has pypdfium2 and Pillow, from the repo root:
#   <python> -m unittest discover -s crates/pdf/tests -p 'test_*.py'
# Clauseo's runtime has both (python-runtime/lock.toml) and pypdf; see src/lib.rs.
import contextlib
import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(__file__), "..", "src", "clauseo_pdf.py")

import pypdfium2 as pdfium  # noqa: E402

try:
    import pypdf
except ImportError:
    pypdf = None


def run(*args, stdin=None, env=None):
    return subprocess.run(
        [sys.executable, SCRIPT, *args], input=stdin, capture_output=True, timeout=120, env=env
    )


def lines(data):
    """Output as text with "\n" line ends (Windows' text-mode stdout writes "\r\n")."""
    return data.decode("utf-8").replace("\r\n", "\n")


def script():
    """The script as a module, for the tests that change a cap or a function before main."""
    spec = importlib.util.spec_from_file_location("clauseo_pdf", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_in(module, *args):
    """main(args) in this process: its exit code and stderr."""
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        code = module.main(list(args))
    return code, err.getvalue()


class PdfTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = self.temp.name

    def tearDown(self):
        self.temp.cleanup()

    def pdf(self, pages, name="doc.pdf", size=(612, 792), sizes=()):
        path = os.path.join(self.dir, name)
        doc = pdfium.PdfDocument.new()
        for each in [size] * pages + list(sizes):
            doc.new_page(*each)
        doc.save(path)
        doc.close()
        return path

    def out(self, name="out"):
        path = os.path.join(self.dir, name)
        os.makedirs(path, exist_ok=True)
        return os.path.join(path, "page")

    def names(self, prefix):
        return sorted(os.listdir(os.path.dirname(prefix)))

    def test_the_version_runs_on_stderr_with_exit_0(self):
        for tool in ("pdftoppm", "pdfinfo"):
            done = run(tool, "-v")
            self.assertEqual(done.returncode, 0)
            self.assertTrue(done.stderr.startswith(f"{tool} version ".encode()), done.stderr)

    def test_pdfinfo_gives_the_page_count_and_size(self):
        done = run("pdfinfo", self.pdf(12))
        self.assertEqual(done.returncode, 0, done.stderr)
        text = lines(done.stdout)
        self.assertRegex(text, r"(?m)^Pages:\s+12$")
        self.assertIn("Page size:      612 x 792 pts (letter)\n", text)
        self.assertIn("Encrypted:      no\n", text)

    def test_pdfinfo_reads_stdin(self):
        with open(self.pdf(3), "rb") as file:
            done = run("pdfinfo", "-", stdin=file.read())
        self.assertRegex(lines(done.stdout), r"(?m)^Pages:\s+3$")

    def test_names_are_padded_by_the_page_count_like_poppler(self):
        for pages, want in ((9, ["page-1.jpg", "page-9.jpg"]),
                            (10, ["page-01.jpg", "page-10.jpg"]),
                            (120, ["page-001.jpg", "page-120.jpg"])):
            prefix = self.out(f"o{pages}")
            done = run("pdftoppm", "-jpeg", "-r", "10", "-f", "1", "-l", "1",
                       self.pdf(pages, f"d{pages}.pdf"), prefix)
            self.assertEqual(done.returncode, 0, done.stderr)
            done = run("pdftoppm", "-jpeg", "-r", "10", "-f", str(pages),
                       os.path.join(self.dir, f"d{pages}.pdf"), prefix)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(self.names(prefix), want)

    def test_a_range_renders_only_its_pages_and_sorts_as_strings(self):
        prefix = self.out()
        done = run("pdftoppm", "-jpeg", "-r", "10", "-f", "8", "-l", "11", self.pdf(12), prefix)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.names(prefix),
                         ["page-08.jpg", "page-09.jpg", "page-10.jpg", "page-11.jpg"])

    def test_a_last_page_past_the_end_stops_at_the_end(self):
        prefix = self.out()
        done = run("pdftoppm", "-png", "-r", "10", "-f", "2", "-l", "99", self.pdf(3), prefix)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.names(prefix), ["page-2.png", "page-3.png"])

    def test_a_first_page_past_the_end_is_poppler_wrong_range(self):
        done = run("pdftoppm", "-jpeg", "-f", "20", self.pdf(12), self.out())
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"Wrong page range given: the first page (20) can not be after the last "
                      b"page (12).", done.stderr)

    def test_the_resolution_scales_the_image(self):
        from PIL import Image

        for dpi, size in (("100", (850, 1100)), ("50", (425, 550)), ("72", (612, 792))):
            prefix = self.out(f"r{dpi}")
            done = run("pdftoppm", "-jpeg", "-r", dpi, self.pdf(1), prefix)
            self.assertEqual(done.returncode, 0, done.stderr)
            with Image.open(prefix + "-1.jpg") as image:
                self.assertEqual(image.size, size)

    def test_jpeg_png_and_single_file_output(self):
        path = self.pdf(2)
        prefix = self.out()
        self.assertEqual(run("pdftoppm", "-jpeg", "-r", "10", path, prefix).returncode, 0)
        self.assertEqual(run("pdftoppm", "-png", "-r", "10", path, prefix).returncode, 0)
        self.assertEqual(run("pdftoppm", "-png", "-r", "10", "-singlefile", path,
                             prefix + "-one").returncode, 0)
        self.assertEqual(self.names(prefix), ["page-1.jpg", "page-1.png", "page-2.jpg",
                                              "page-2.png", "page-one.png"])
        with open(prefix + "-1.jpg", "rb") as file:
            self.assertEqual(file.read(3), b"\xff\xd8\xff")
        with open(prefix + "-1.png", "rb") as file:
            self.assertEqual(file.read(8), b"\x89PNG\r\n\x1a\n")

    def test_stdin_input_renders(self):
        prefix = self.out()
        with open(self.pdf(2), "rb") as file:
            done = run("pdftoppm", "-jpeg", "-r", "10", "-", prefix, stdin=file.read())
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.names(prefix), ["page-1.jpg", "page-2.jpg"])

    def test_a_missing_file_is_poppler_open_error(self):
        missing = os.path.join(self.dir, "missing.pdf")
        for tool, args in (("pdftoppm", [missing, self.out()]), ("pdfinfo", [missing])):
            done = run(tool, *args)
            self.assertEqual(done.returncode, 1)
            self.assertIn(f"I/O Error: Couldn't open file '{missing}': ".encode(), done.stderr)

    def test_a_damaged_file_is_an_xref_error(self):
        path = os.path.join(self.dir, "bad.pdf")
        with open(path, "wb") as file:
            file.write(b"not a pdf at all")
        done = run("pdfinfo", path)
        self.assertEqual(done.returncode, 1)
        self.assertIn(b"Syntax Error: Couldn't read xref table", done.stderr)

    @unittest.skipIf(pypdf is None, "pypdf is not installed")
    def test_an_encrypted_file_needs_its_password(self):
        writer = pypdf.PdfWriter(clone_from=self.pdf(2, "plain.pdf"))
        writer.encrypt("secret", algorithm="RC4-128")
        path = os.path.join(self.dir, "locked.pdf")
        with open(path, "wb") as file:
            writer.write(file)
        for tool, args in (("pdftoppm", [path, self.out()]), ("pdfinfo", [path])):
            done = run(tool, *args)
            self.assertEqual(done.returncode, 1)
            self.assertIn(b"Command Line Error: Incorrect password", done.stderr)
        done = run("pdfinfo", "-upw", "secret", path)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(b"Encrypted:      yes", done.stdout)

    def test_a_run_over_the_page_cap_fails_before_rendering(self):
        prefix = self.out()
        done = run("pdftoppm", "-jpeg", self.pdf(201), prefix)
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"at most 200 pages", done.stderr)
        self.assertEqual(self.names(prefix), [])

    def test_a_page_over_the_pixel_cap_fails(self):
        done = run("pdftoppm", "-png", "-r", "2000", self.pdf(1), self.out())
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"give a lower -r", done.stderr)

    def test_a_sliver_page_is_refused_before_rendering(self):
        # 36,000,000 x 0.3 pt at -r 100: PDFium would make a 50,000,000 x 1 bitmap (150 MB);
        # rounding the short side to the nearest pixel made it 0 and let it through.
        prefix = self.out()
        done = run("pdftoppm", "-png", "-r", "100", self.pdf(0, sizes=[(36_000_000, 0.3)]),
                   prefix)
        self.assertEqual(done.returncode, 99, done.stderr)
        self.assertIn(b"page 1 (3.6e+07 x 0.3 pts) would render too large", done.stderr)
        self.assertEqual(self.names(prefix), [])
        # Under the side cap but over the pixel cap once each side is rounded up.
        done = run("pdftoppm", "-png", "-r", "72", self.pdf(0, "w.pdf", sizes=[(65_000, 1600)]),
                   prefix)
        self.assertEqual(done.returncode, 99, done.stderr)
        self.assertIn(b"would render 65000 x 1600 pixels", done.stderr)

    def test_asymmetric_rx_ry_are_checked_at_the_scale_the_render_uses(self):
        # The render uses max(rx, ry) for both sides: 11,051 x 14,301 pixels here (PDFium rounds
        # 11,050.000…1 up), though the old per-side check saw 11,050 x 11.
        prefix = self.out()
        done = run("pdftoppm", "-png", "-rx", "1300", "-ry", "1", self.pdf(1), prefix)
        self.assertEqual(done.returncode, 99, done.stderr)
        self.assertIn(b"would render 11051 x 14301 pixels", done.stderr)
        self.assertEqual(self.names(prefix), [])
        from PIL import Image

        done = run("pdftoppm", "-png", "-rx", "20", "-ry", "10", self.pdf(1, "s.pdf"), prefix)
        self.assertEqual(done.returncode, 0, done.stderr)
        with Image.open(prefix + "-1.png") as image:
            self.assertEqual(image.size, (170, 110))

    def test_a_resolution_that_is_not_finite_is_a_usage_error(self):
        path = self.pdf(1)
        for opt, value in (("-r", "nan"), ("-r", "inf"), ("-rx", "-inf"), ("-ry", "NaN")):
            done = run("pdftoppm", "-png", opt, value, path, self.out())
            self.assertEqual(done.returncode, 99, (opt, value))
            self.assertIn(b"Usage: pdftoppm", done.stderr)
            self.assertNotIn(b"Traceback", done.stderr)

    def test_a_later_page_refused_removes_the_pages_the_run_wrote(self):
        prefix = self.out()
        path = self.pdf(2, sizes=[(36_000_000, 0.3)])
        done = run("pdftoppm", "-png", "-r", "10", path, prefix)
        self.assertEqual(done.returncode, 99, done.stderr)
        self.assertIn(b"page 3 ", done.stderr)
        self.assertEqual(self.names(prefix), [])

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root writes read-only files")
    def test_a_read_only_file_in_the_way_fails_the_run_and_survives(self):
        prefix = os.path.join(self.dir, "keep")
        with open(prefix + "-1.png", "wb") as file:
            file.write(b"mine")
        os.chmod(prefix + "-1.png", 0o444)
        done = run("pdftoppm", "-png", "-r", "10", self.pdf(1), prefix)
        self.assertEqual(done.returncode, 2, done.stderr)
        self.assertIn(b"Permission denied", done.stderr)
        with open(prefix + "-1.png", "rb") as file:
            self.assertEqual(file.read(), b"mine")

    def test_a_failed_run_leaves_the_files_it_overwrote_and_removes_the_ones_it_made(self):
        prefix = self.out()
        with open(prefix + "-1.png", "wb") as file:
            file.write(b"old")
        done = run("pdftoppm", "-png", "-r", "10", self.pdf(2, sizes=[(36_000_000, 0.3)]),
                   prefix)
        self.assertEqual(done.returncode, 99, done.stderr)
        self.assertIn(b"page 3 ", done.stderr)
        self.assertEqual(self.names(prefix), ["page-1.png"])
        with open(prefix + "-1.png", "rb") as file:
            self.assertEqual(file.read(8), b"\x89PNG\r\n\x1a\n")

    def test_the_byte_cap_stops_the_run_and_removes_its_pages(self):
        module = script()
        self.assertEqual(module.MAX_BYTES, 1_000_000_000)
        # Each page is 612 x 792 x 3 bytes of PPM (1.45 MB) plus its header.
        module.MAX_BYTES = 3_000_000
        prefix = self.out()
        code, err = run_in(module, "pdftoppm", "-r", "72", self.pdf(3), prefix)
        self.assertEqual(code, 99, err)
        self.assertIn("writes at most 3000000 bytes a run", err)
        self.assertEqual(self.names(prefix), [])
        code, err = run_in(module, "pdftoppm", "-r", "72", "-l", "2", self.pdf(3, "b.pdf"),
                           prefix)
        self.assertEqual(code, 0, err)
        self.assertEqual(self.names(prefix), ["page-1.ppm", "page-2.ppm"])

    def test_a_page_that_fails_to_render_is_one_line_and_exit_99(self):
        module = script()

        def broken(*_):
            raise pdfium.PdfiumError("Failed to load page.")

        prefix = self.out()
        module.render = broken
        code, err = run_in(module, "pdftoppm", "-png", self.pdf(2), prefix)
        self.assertEqual((code, err), (99, "Internal Error: pdftoppm could not read or render "
                                           "the document: PdfiumError: Failed to load page.\n"))

        def no_memory(*_):
            raise MemoryError

        module.render = no_memory
        code, err = run_in(module, "pdftoppm", "-png", self.pdf(2, "m.pdf"), prefix)
        self.assertEqual((code, err), (99, "Internal Error: pdftoppm could not read or render "
                                           "the document: MemoryError\n"))

    def test_pdfinfo_lists_at_most_the_page_cap(self):
        path = self.pdf(201)
        done = run("pdfinfo", "-f", "1", "-l", "201", path)
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"this pdfinfo lists at most 200 pages a run", done.stderr)
        self.assertEqual(done.stdout, b"")
        done = run("pdfinfo", "-f", "2", path)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertIn(b"Page  201 size:", done.stdout)
        self.assertRegex(lines(run("pdfinfo", path).stdout), r"(?m)^Pages:\s+201$")

    @unittest.skipIf(pypdf is None, "pypdf is not installed")
    def test_hindi_text_and_names_come_out_as_utf8_under_a_cp1252_console(self):
        # Windows' console code page: PYTHONIOENCODING stands in for it here (the launcher's
        # `-I` ignores the variable, so a Windows run gets cp1252 without it).
        env = dict(os.environ, PYTHONIOENCODING="cp1252", PYTHONUTF8="0")
        writer = pypdf.PdfWriter(clone_from=self.pdf(1, "plain.pdf"))
        writer.add_metadata({"/Title": "अनुबंध", "/Author": "Zoë"})
        path = os.path.join(self.dir, "दस्तावेज़.pdf")
        with open(path, "wb") as file:
            writer.write(file)
        done = run("pdfinfo", path, env=env)
        self.assertEqual(done.returncode, 0, done.stderr)
        text = lines(done.stdout)
        self.assertIn("Title:          अनुबंध\n", text)
        self.assertIn("Author:         Zoë\n", text)
        missing = os.path.join(self.dir, "गुम.pdf")
        done = run("pdftoppm", "-png", missing, self.out(), env=env)
        self.assertEqual(done.returncode, 1)
        self.assertIn(f"I/O Error: Couldn't open file '{missing}': ", done.stderr.decode("utf-8"))
        prefix = os.path.join(self.dir, "पृष्ठ")
        done = run("pdftoppm", "-png", "-r", "10", path, prefix, env=env)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(os.path.exists(prefix + "-1.png"))

    def test_an_unknown_option_is_a_usage_error(self):
        done = run("pdftoppm", "-tiff", self.pdf(1), self.out())
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"Usage: pdftoppm", done.stderr)


if __name__ == "__main__":
    unittest.main()
