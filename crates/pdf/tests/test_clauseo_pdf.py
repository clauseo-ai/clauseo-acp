# Tests of src/clauseo_pdf.py against PDFs made here with pypdfium2 (and pypdf, for the encrypted
# one). Run with an interpreter that has pypdfium2 and Pillow, from the repo root:
#   <python> -m unittest discover -s crates/pdf/tests -p 'test_*.py'
# Clauseo's runtime has both (python-runtime/lock.toml) and pypdf; see src/lib.rs.
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


def run(*args, stdin=None):
    return subprocess.run(
        [sys.executable, SCRIPT, *args], input=stdin, capture_output=True, timeout=120
    )


class PdfTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = self.temp.name

    def tearDown(self):
        self.temp.cleanup()

    def pdf(self, pages, name="doc.pdf", size=(612, 792)):
        path = os.path.join(self.dir, name)
        doc = pdfium.PdfDocument.new()
        for _ in range(pages):
            doc.new_page(*size)
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
        text = done.stdout.decode()
        self.assertRegex(text, r"(?m)^Pages:\s+12$")
        self.assertIn("Page size:      612 x 792 pts (letter)\n", text)
        self.assertIn("Encrypted:      no\n", text)

    def test_pdfinfo_reads_stdin(self):
        with open(self.pdf(3), "rb") as file:
            done = run("pdfinfo", "-", stdin=file.read())
        self.assertRegex(done.stdout.decode(), r"(?m)^Pages:\s+3$")

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

    def test_an_unknown_option_is_a_usage_error(self):
        done = run("pdftoppm", "-tiff", self.pdf(1), self.out())
        self.assertEqual(done.returncode, 99)
        self.assertIn(b"Usage: pdftoppm", done.stderr)


if __name__ == "__main__":
    unittest.main()
