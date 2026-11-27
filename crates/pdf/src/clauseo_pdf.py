# Clauseo's own `pdftoppm` and `pdfinfo`, on pypdfium2 (PDFium) and Pillow: no poppler code.
# The launcher (crates/pdf/src/lib.rs) runs it as `python -I -c <this file> <tool> <args>`.
# Claude Code's Read renders long PDFs with `pdftoppm -jpeg -r 100 [-f N] [-l M] <pdf> <prefix>`
# and counts pages with `pdfinfo`'s `Pages:` line; Codex is told to run pdftoppm itself. Names,
# padding, messages and exit codes follow poppler's (utils/pdftoppm.cc, pdfinfo.cc), so Claude
# Code's error matching reads them. Caps, each checked before the work it bounds: a run covers
# at most MAX_PAGES pages (pdftoppm renders them, pdfinfo -f/-l lists them); a page renders to at
# most MAX_PIXELS pixels and MAX_SIDE a side, as PDFium will size it; a run writes at most
# MAX_BYTES of images. A run refused part way removes the pages it wrote. So a 2,000-page bundle,
# a huge -r or a sliver page fails fast instead of filling the memory or the disk.
import math
import os
import sys

MAX_PAGES = 200
MAX_PIXELS = 100_000_000
MAX_SIDE = 65_500
MAX_BYTES = 1_000_000_000
OPEN_ERROR, OUTPUT_ERROR, OTHER_ERROR = 1, 2, 99


class Fail(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


# Each tool's options: name -> kind (flag, int, float or str).
PDFTOPPM = {
    "-f": "int", "-l": "int", "-o": "flag", "-e": "flag", "-singlefile": "flag",
    "-forcenum": "flag", "-r": "float", "-rx": "float", "-ry": "float", "-scale-to": "int",
    "-png": "flag", "-jpeg": "flag", "-jpegopt": "str", "-gray": "flag", "-sep": "str",
    "-opw": "str", "-upw": "str", "-q": "flag", "-v": "flag", "-h": "flag", "-help": "flag",
    "--help": "flag", "-?": "flag",
    # Render the crop box, not the media box: what PDFium renders anyway. Claude Code 2.1.331's
    # Read passes it, with the PDF on stdin (seen on Windows, 27 Nov).
    "-cropbox": "flag",
}
PDFINFO = {
    "-f": "int", "-l": "int", "-isodates": "flag", "-rawdates": "flag", "-opw": "str",
    "-upw": "str", "-v": "flag", "-h": "flag", "-help": "flag", "--help": "flag", "-?": "flag",
}
USAGE = {
    "pdftoppm": "Usage: pdftoppm [options] [PDF-file [PPM-file-prefix]]\n  "
    + " ".join(sorted(PDFTOPPM)),
    "pdfinfo": "Usage: pdfinfo [options] <PDF-file>\n  " + " ".join(sorted(PDFINFO)),
}


def parse(tool, table, args):
    """Poppler-style options anywhere among the arguments; `-` alone is a file (stdin)."""
    opts, files, i = {}, [], 0
    while i < len(args):
        arg = args[i]
        if arg == "--":
            files.extend(args[i + 1:])
            break
        if arg == "-" or not arg.startswith("-"):
            files.append(arg)
        elif arg not in table:
            raise Fail(OTHER_ERROR, f"{version(tool)}\n{USAGE[tool]}")
        elif table[arg] == "flag":
            opts[arg] = True
        else:
            if i + 1 >= len(args):
                raise Fail(OTHER_ERROR, f"{version(tool)}\n{USAGE[tool]}")
            i += 1
            try:
                kind = {"int": int, "float": float, "str": str}[table[arg]]
                opts[arg] = kind(args[i])
                if kind is float and not math.isfinite(opts[arg]):
                    raise ValueError(args[i])
            except ValueError:
                raise Fail(OTHER_ERROR, f"{version(tool)}\n{USAGE[tool]}") from None
        i += 1
    return opts, files


def version(tool):
    try:
        from pypdfium2.version import PDFIUM_INFO, PYPDFIUM_INFO
        engine = f"pypdfium2 {PYPDFIUM_INFO}, PDFium {PDFIUM_INFO}"
    except ImportError:
        engine = "pypdfium2 missing"
    return f"{tool} version clauseo-1 ({engine}; not poppler)"


def open_doc(src, opts):
    """The document and its size in bytes; poppler's messages and exit 1 when it can't open."""
    import pypdfium2 as pdfium
    import pypdfium2.raw as raw

    if src in ("-", "fd://0"):
        data = sys.stdin.buffer.read()
        size, stream = len(data), data
    else:
        try:
            stream = open(src, "rb")
            size = os.fstat(stream.fileno()).st_size
        except OSError as err:
            reason = err.strerror or str(err)
            raise Fail(OPEN_ERROR, f"I/O Error: Couldn't open file '{src}': {reason}.") from None
    last = None
    for password in [opts.get("-upw"), opts.get("-opw")] if (
        "-upw" in opts or "-opw" in opts
    ) else [None]:
        try:
            if not isinstance(stream, bytes):
                stream.seek(0)
            return pdfium.PdfDocument(stream, password=password, autoclose=True), size
        except pdfium.PdfiumError as err:
            last = err
    if getattr(last, "err_code", None) == raw.FPDF_ERR_PASSWORD:
        raise Fail(OPEN_ERROR, "Command Line Error: Incorrect password")
    raise Fail(OPEN_ERROR, "Syntax Error: Couldn't read xref table")


def page_range(opts, count, single=False):
    """Poppler's: -f below 1 is 1, -l past the end (or unset) is the last page."""
    first = max(opts.get("-f", 1), 1)
    last = opts.get("-l", 0)
    if single and last < 1:
        last = first
    if last < 1 or last > count:
        last = count
    if last < first:
        raise Fail(
            OTHER_ERROR,
            f"Wrong page range given: the first page ({first}) can not be after the last page "
            f"({last}).",
        )
    return first, last


def pdftoppm(args):
    opts, files = parse("pdftoppm", PDFTOPPM, args)
    if any(k in opts for k in ("-v", "-h", "-help", "--help", "-?")):
        text = version("pdftoppm")
        if "-v" not in opts:
            text += "\n" + USAGE["pdftoppm"]
        print(text, file=sys.stderr)
        return 0
    if len(files) > 2:
        raise Fail(OTHER_ERROR, f"{version('pdftoppm')}\n{USAGE['pdftoppm']}")
    src = files[0] if files else "-"
    root = files[1] if len(files) == 2 else None
    doc, _ = open_doc(src, opts)
    count = len(doc)
    single = opts.get("-singlefile", False)
    first, last = page_range(opts, count, single)
    if single and first < last:
        if not opts.get("-q"):
            print(f"Warning: Single file will write only the first of the {last + 1 - first} "
                  "pages.", file=sys.stderr)
        last = first
    pages = [
        pg for pg in range(first, last + 1)
        if not (opts.get("-e") and pg % 2 == 1) and not (opts.get("-o") and pg % 2 == 0)
    ]
    if not pages:
        raise Fail(OTHER_ERROR, "Invalid even/odd page selection, no pages match criteria.")
    if len(pages) > MAX_PAGES:
        raise Fail(
            OTHER_ERROR,
            f"Command Line Error: this pdftoppm renders at most {MAX_PAGES} pages a run, and "
            f"pages {first}-{last} are {len(pages)}; give a smaller range with -f and -l.",
        )
    png, jpeg, gray = opts.get("-png"), opts.get("-jpeg"), opts.get("-gray")
    ext = "png" if png else "jpg" if jpeg else "pgm" if gray else "ppm"
    width = len(str(count))
    sep = opts.get("-sep", "-")
    quality = jpeg_quality(opts.get("-jpegopt", ""))
    out = Output()
    try:
        for pg in pages:
            image = render(doc, pg, opts)
            if root is None:
                path = None
            elif single and not opts.get("-forcenum"):
                path = f"{root}.{ext}"
            else:
                path = f"{root}{sep}{pg:0{width}d}.{ext}"
            out.save(image, path, ext, quality)
    except Exception:
        out.remove()
        raise
    doc.close()
    return 0


def jpeg_quality(text):
    for part in filter(None, text.split(",")):
        key, _, value = part.partition("=")
        if key == "quality" and value.isdigit() and 0 <= int(value) <= 100:
            return int(value)
        if key not in ("quality", "progressive", "optimize"):
            raise Fail(OTHER_ERROR, f"Unknown jpeg option \"{key}\"")
    return 75


def bitmap_size(pg, w, h, scale):
    """The bitmap PDFium makes for a w x h pt page at `scale` (pypdfium2's PdfPage.render: each
    side rounded up), or Fail when it is empty, not finite, or over a cap."""
    sw, sh = w * scale, h * scale
    if not all(math.isfinite(v) for v in (w, h, sw, sh)) or sw > MAX_SIDE or sh > MAX_SIDE:
        size = "too large"
    else:
        cw, ch = math.ceil(sw), math.ceil(sh)
        if cw >= 1 and ch >= 1 and cw * ch <= MAX_PIXELS:
            return cw, ch
        size = f"{cw} x {ch} pixels"
    raise Fail(
        OTHER_ERROR,
        f"Command Line Error: page {pg} ({w:g} x {h:g} pts) would render {size}; this pdftoppm "
        f"renders 1 to {MAX_PIXELS} pixels a page, at most {MAX_SIDE} a side; give a lower -r.",
    )


def render(doc, pg, opts):
    page = doc[pg - 1]
    try:
        w, h = page.get_size()
        rx = opts.get("-rx", opts.get("-r", 150.0))
        ry = opts.get("-ry", opts.get("-r", 150.0))
        if "-scale-to" in opts and w > 0 and h > 0:
            rx = ry = 72.0 * opts["-scale-to"] / max(w, h)
        if not rx > 0 or not ry > 0:
            raise Fail(OTHER_ERROR, "Command Line Error: the resolution must be above 0")
        # The one scale the render uses, for both sides; -rx/-ry differ only by the resize.
        scale = max(rx, ry) / 72
        bitmap_size(pg, w, h, scale)
        px, py = round(w * rx / 72), round(h * ry / 72)
        bitmap = page.render(scale=scale, grayscale=bool(opts.get("-gray")))
        image = bitmap.to_pil()
        bitmap.close()
    finally:
        page.close()
    if rx != ry:
        image = image.resize((max(px, 1), max(py, 1)))
    return image.convert("L") if opts.get("-gray") else image.convert("RGB")


class Output:
    """A run's image writes: at most MAX_BYTES in all, counted as written (a write that would
    pass the cap fails before it lands), and the files it made, for `remove`."""

    def __init__(self):
        self.written, self.paths, self.file = 0, [], None

    def write(self, data):
        self.written += len(data)
        if self.written > MAX_BYTES:
            raise Fail(
                OTHER_ERROR,
                f"Command Line Error: this pdftoppm writes at most {MAX_BYTES} bytes a run; "
                "give a smaller range with -f and -l, or a lower -r.",
            )
        return self.file.write(data)

    def flush(self):
        self.file.flush()

    def save(self, image, path, ext, quality):
        fmt = {"png": "PNG", "jpg": "JPEG", "pgm": "PPM", "ppm": "PPM"}[ext]
        extra = {"quality": quality} if fmt == "JPEG" else {}
        try:
            if path is None:
                self.file = sys.stdout.buffer
                image.save(self, fmt, **extra)
                self.flush()
                return
            # Only a file this run made is removed on failure: one it overwrote stays, as
            # poppler leaves it, and one it could not open was never its own.
            new = not os.path.lexists(path)
            with open(path, "wb") as self.file:
                if new:
                    self.paths.append(path)
                image.save(self, fmt, **extra)
        except OSError as err:
            reason = err.strerror or str(err)
            message = f"I/O Error: Couldn't write file '{path}': {reason}."
            raise Fail(OUTPUT_ERROR, message) from None

    def remove(self):
        for path in self.paths:
            try:
                os.remove(path)
            except OSError:
                pass


def size_name(w, h):
    w, h = min(w, h), max(w, h)
    for name, (pw, ph) in (("letter", (612, 792)), ("legal", (612, 1008)),
                           ("A5", (420, 595)), ("A4", (595, 842)), ("A3", (842, 1191))):
        if abs(w - pw) < 1 and abs(h - ph) < 1:
            return f" ({name})"
    return ""


def pdf_date(text, opts):
    """`D:YYYYMMDDHHmmSS+hh'mm'` as poppler's -isodates writes it; raw with -rawdates."""
    if opts.get("-rawdates") or not text.startswith("D:"):
        return text
    s = text[2:]
    digits = "".join(c for c in s[:14] if c.isdigit())
    if len(digits) < 4:
        return text
    digits = (digits + "0101000000"[len(digits) - 4:])[:14]
    out = f"{digits[:4]}-{digits[4:6]}-{digits[6:8]}T{digits[8:10]}:{digits[10:12]}:{digits[12:]}"
    zone = s[14:].replace("'", "")
    if zone[:1] == "Z":
        return out + "Z"
    if zone[:1] in "+-" and len(zone) >= 3:
        return out + zone[:3] + (":" + zone[3:5] if len(zone) >= 5 else "")
    return out


def pdfinfo(args):
    import pypdfium2.raw as raw

    opts, files = parse("pdfinfo", PDFINFO, args)
    if any(k in opts for k in ("-v", "-h", "-help", "--help", "-?")):
        text = version("pdfinfo")
        if "-v" not in opts:
            text += "\n" + USAGE["pdfinfo"]
        print(text, file=sys.stderr)
        return 0
    if len(files) != 1:
        raise Fail(OTHER_ERROR, f"{version('pdfinfo')}\n{USAGE['pdfinfo']}")
    doc, size = open_doc(files[0], opts)
    lines = []
    meta = doc.get_metadata_dict(skip_empty=True)
    for key in ("Title", "Subject", "Keywords", "Author", "Creator", "Producer"):
        if key in meta:
            lines.append((key, meta[key]))
    for key in ("CreationDate", "ModDate"):
        if key in meta:
            lines.append((key, pdf_date(meta[key], opts)))
    count = len(doc)
    lines.append(("Pages", str(count)))
    encrypted = raw.FPDF_GetSecurityHandlerRevision(doc.raw) != -1
    lines.append(("Encrypted", "yes" if encrypted else "no"))
    if count and "-f" not in opts and "-l" not in opts:
        first = last = 1
    elif count:
        first, last = page_range(opts, count)
    else:
        first, last = 1, 0
    if last + 1 - first > MAX_PAGES:
        raise Fail(
            OTHER_ERROR,
            f"Command Line Error: this pdfinfo lists at most {MAX_PAGES} pages a run, and pages "
            f"{first}-{last} are {last + 1 - first}; give a smaller range with -f and -l.",
        )
    for pg in range(first, last + 1):
        page = doc[pg - 1]
        w, h = page.get_size()
        rot = page.get_rotation()
        page.close()
        if "-f" in opts or "-l" in opts:
            lines.append((f"Page {pg:4d} size", f"{w:g} x {h:g} pts{size_name(w, h)}"))
            lines.append((f"Page {pg:4d} rot", str(rot)))
        else:
            lines.append(("Page size", f"{w:g} x {h:g} pts{size_name(w, h)}"))
            lines.append(("Page rot", str(rot)))
    lines.append(("File size", f"{size} bytes"))
    ver = doc.get_version()
    if ver:
        lines.append(("PDF version", f"{ver // 10}.{ver % 10}"))
    doc.close()
    out = "".join(f"{key + ':':<16}{value}\n" for key, value in lines)
    sys.stdout.write(out)
    return 0


def failures():
    """The errors main reports as one line: PDFium's, memory, and bad numbers."""
    kinds = [MemoryError, ValueError, OverflowError]
    try:
        from pypdfium2 import PdfiumError
        kinds.append(PdfiumError)
    except ImportError:
        pass
    return tuple(kinds)


def utf8_text():
    """UTF-8 on stdout and stderr whatever the console's code page: `-I` ignores PYTHONUTF8 and
    PYTHONIOENCODING, and a Windows console's cp1252 cannot write a Hindi title or file name.
    Claude Code and Codex read both as UTF-8."""
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")


def main(argv):
    utf8_text()
    tool = argv[0] if argv else ""
    run = {"pdftoppm": pdftoppm, "pdfinfo": pdfinfo}.get(tool)
    if run is None:
        print(f"clauseo-pdf: unknown tool {tool!r}", file=sys.stderr)
        return OTHER_ERROR
    try:
        return run(argv[1:])
    except Fail as fail:
        print(fail.message, file=sys.stderr)
        return fail.code
    except ImportError as err:
        print(f"Internal Error: {tool} needs Clauseo's Python runtime ({err})", file=sys.stderr)
        return OTHER_ERROR
    except failures() as err:
        # A page that will not load, a bitmap that cannot be made, memory run out: one line.
        print(f"Internal Error: {tool} could not read or render the document: "
              f"{type(err).__name__}: {err}".rstrip(": "), file=sys.stderr)
        return OTHER_ERROR
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    # `python -c` puts "-c" first; a file run puts the file's path first.
    sys.exit(main(sys.argv[1:]))
