# Clauseo's own `pdftotext`, on pypdfium2 (PDFium) and the Python runtime's tesseract: no poppler.
# The launcher (crates/pdf/src/lib.rs) runs it as `python -I -X utf8 -c <this file> pdftotext
# <args>`. A page whose text layer has MIN_CHARS characters or more prints that; any other page is
# OCR'd: rendered by PDFium at DPI as a grayscale PGM, which goes to tesseract on stdin (never a
# path or bytes from the document: given anything but an image, tesseract reads it as a list of
# file paths), with --tessdata-dir beside the program and no TESSDATA_PREFIX. One run reads text
# and OCRs until BUDGET seconds after it started (at most MAX_TEXT and MAX_OCR pages) and writes
# its notes until END, inside Claude Code's 120 s Bash kill; the rest are named in a marker line.
# With CLAUSEO_OCR_DIR (the matter's notes/ocr) the pages are kept in .by-hash/<sha256>-<settings>/
# there, so a rerun continues, a renamed file is found by its bytes, and a changed byte misses;
# a lock there makes one process do the OCR while another waits for its pages. A document under
# the current folder also gets <rel path>.txt there: the text of every page known so far.
import hashlib
import json
import os
import sys
import threading
import time

MIN_CHARS = 24
BUDGET = 100.0
END = 110.0
MAX_TEXT = 10_000
MAX_OCR = 1000
DPI = 300
MAX_PIXELS = 20_000_000
STALE = 30.0
FORMAT = 1
OPEN_ERROR, OUTPUT_ERROR, OTHER_ERROR = 1, 2, 99
OPTIONS = {
    "-f": int, "-l": int, "-layout": None, "-enc": str, "-nopgbrk": None, "-q": None,
    "-v": None, "-h": None, "-help": None, "--help": None, "-?": None,
}
USAGE = "Usage: pdftotext [options] <PDF-file> [<text-file>|-]\n  " + " ".join(sorted(OPTIONS))
START = time.monotonic()


class Fail(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code, self.message = code, message


def version():
    try:
        from pypdfium2.version import PDFIUM_INFO, PYPDFIUM_INFO
        engine = f"pypdfium2 {PYPDFIUM_INFO}, PDFium {PDFIUM_INFO}"
    except ImportError:
        engine = "pypdfium2 missing"
    return f"pdftotext version clauseo-1 ({engine}, tesseract; not poppler)"


def parse(args):
    opts, files, i = {}, [], 0
    while i < len(args):
        arg = args[i]
        if arg == "--":
            files.extend(args[i + 1:])
            break
        if arg == "-" or not arg.startswith("-"):
            files.append(arg)
        elif arg not in OPTIONS:
            raise Fail(OTHER_ERROR, f"{version()}\n{USAGE}")
        elif OPTIONS[arg] is None:
            opts[arg] = True
        else:
            i += 1
            try:
                opts[arg] = OPTIONS[arg](args[i])
            except (IndexError, ValueError):
                raise Fail(OTHER_ERROR, f"{version()}\n{USAGE}") from None
        i += 1
    return opts, files


def spans(pages, word=True):
    """[3, 4, 5, 9] -> "pages 3-5, 9"; [3] -> "page 3"."""
    out, pages = [], sorted(pages)
    for i, pg in enumerate(pages):
        if i and pg == pages[i - 1] + 1:
            out[-1][1] = pg
        else:
            out.append([pg, pg])
    text = ", ".join(f"{a}" if a == b else f"{a}-{b}" for a, b in out)
    return (("page " if len(pages) == 1 else "pages ") if word else "") + text


def runtime(exe=None, windows=None):
    """tesseract, its tessdata folder and the runtime build's name, from this interpreter's
    runtime: <rt>/bin/{python3,tesseract,tessdata} on macOS, <rt>\\python.exe with
    <rt>\\clauseo-bin\\{tesseract.exe,tessdata} on Windows."""
    exe = os.path.abspath(exe or sys.executable)
    if os.name == "nt" if windows is None else windows:
        root = os.path.dirname(exe)
        folder, name = os.path.join(root, "clauseo-bin"), "tesseract.exe"
    else:
        folder, name = os.path.dirname(exe), "tesseract"
        root = os.path.dirname(folder)
    try:
        with open(os.path.join(root, "clauseo-python.json"), encoding="utf-8") as file:
            build = str(json.load(file).get("archive", "?"))
    except (OSError, ValueError, AttributeError):
        build = "?"
    return os.path.join(folder, name), os.path.join(folder, "tessdata"), build


def engine_env():
    """tesseract's environment: TESSDATA_PREFIX out (its models are --tessdata-dir's only), one
    thread each (the workers are the parallelism)."""
    env = {k: v for k, v in os.environ.items() if k.upper() != "TESSDATA_PREFIX"}
    env["OMP_THREAD_LIMIT"] = "1"
    return env


def engine_command(program, tessdata, langs):
    return [program, "stdin", "stdout", "-l", langs, "--tessdata-dir", tessdata]


def write_atomic(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = f"{path}.{os.getpid()}.{threading.get_ident()}.tmp"
    with open(tmp, "wb") as file:
        file.write(data)
    for wait in (0.05, 0.2, 0.5, None):
        try:
            return os.replace(tmp, path)
        except PermissionError:
            # Windows refuses to replace a file another process has open (a waiter reading it).
            if wait is None:
                os.remove(tmp)
                raise
            time.sleep(wait)


def keep(path, body):
    """A cache write; one that fails costs a later run the work, not this run its output."""
    try:
        write_atomic(path, json.dumps(body).encode())
    except OSError:
        pass


def read_json(path):
    try:
        with open(path, encoding="utf-8") as file:
            value = json.load(file)
        return value if isinstance(value, dict) else None
    except (OSError, ValueError):
        return None


class Cache:
    """One document's pages for one engine setting, in <ocr>/.by-hash/<sha256>-<settings>/:
    pNNNN.json per page ({"kind": "text"|"ocr", "langs", "text"}), meta.json (the language probe's
    answer, the engine), failed.json (why it could not be opened) and lock."""

    def __init__(self, root, digest, settings):
        tag = hashlib.sha256(settings.encode()).hexdigest()[:8]
        self.root, self.digest = root, digest
        self.dir = os.path.join(root, ".by-hash", f"{digest}-{tag}")
        self.lock_path = os.path.join(self.dir, "lock")
        self.beat = None

    def page(self, pg):
        found = read_json(os.path.join(self.dir, f"p{pg:04d}.json"))
        return found if found and isinstance(found.get("text"), str) else None

    def put(self, pg, kind, text, langs=""):
        body = {"kind": kind, "langs": langs, "text": text}
        if self.page(pg) != body:
            keep(os.path.join(self.dir, f"p{pg:04d}.json"), body)

    def meta(self):
        return read_json(os.path.join(self.dir, "meta.json")) or {}

    def set_meta(self, **values):
        meta = self.meta()
        meta.update(values)
        keep(os.path.join(self.dir, "meta.json"), meta)

    def failed(self):
        return read_json(os.path.join(self.dir, "failed.json"))

    def fail(self, reason):
        keep(os.path.join(self.dir, "failed.json"),
             {"reason": reason, "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())})

    def lock(self):
        """True when this process now holds the lock (O_EXCL; a holder touches it every few
        seconds, so one untouched for STALE seconds is a dead holder's and is taken over)."""
        os.makedirs(self.dir, exist_ok=True)
        for _ in range(2):
            try:
                fd = os.open(self.lock_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
            except FileExistsError:
                try:
                    if time.time() - os.stat(self.lock_path).st_mtime < STALE:
                        return False
                    os.remove(self.lock_path)
                except OSError:
                    pass
                continue
            with os.fdopen(fd, "w") as file:
                file.write(json.dumps({"pid": os.getpid(), "at": time.time()}))
            self.beat = threading.Event()
            threading.Thread(target=self.heartbeat, args=(self.beat,), daemon=True).start()
            return True
        return False

    def heartbeat(self, stop):
        while not stop.wait(2.0):
            try:
                os.utime(self.lock_path)
            except OSError:
                pass

    def unlock(self):
        if self.beat:
            self.beat.set()
            self.beat = None
            try:
                os.remove(self.lock_path)
            except OSError:
                pass


def mirror_path(root, src, cwd, path=os.path):
    """<root>/<src relative to cwd>.txt, or None for a document outside cwd (another drive, or
    up and out)."""
    try:
        rel = path.relpath(path.normpath(path.join(cwd, src)), cwd)
    except ValueError:
        return None, None
    if path.isabs(rel) or rel == path.pardir or rel.startswith(path.pardir + path.sep):
        return None, None
    return path.join(root, rel + ".txt"), rel


def is_devanagari(text):
    """Whether 2 % or more of the letters are Devanagari (None: too few letters to say)."""
    letters = [c for c in text if c.isalpha() or "\u0900" <= c <= "\u097f"]
    if len(letters) < 10:
        return None
    return sum("\u0900" <= c <= "\u097f" for c in letters) >= 0.02 * len(letters)


def layout(textpage, plain):
    """The page's text with its lines and columns kept, poppler -layout's way: each run of text
    PDFium reports goes on its row (by height) at its column (by x over the median char width)."""
    runs = []
    for i in range(textpage.count_rects()):
        left, bottom, right, top = textpage.get_rect(i)
        text = textpage.get_text_bounded(left, bottom, right, top).replace("\r", "")
        text = text.replace("\n", " ")
        if text.strip():
            runs.append((top, bottom, left, (right - left) / max(len(text), 1), text))
    if not runs:
        return plain
    widths = sorted(run[3] for run in runs if run[3] > 0) or [6.0]
    width = widths[len(widths) // 2]
    rows = []
    for top, bottom, left, _, text in sorted(runs, key=lambda r: (-r[0], r[2])):
        middle = (top + bottom) / 2
        if rows and abs(rows[-1][0] - middle) < max((top - bottom) / 2, 1):
            rows[-1][1].append((left, text))
        else:
            rows.append([middle, [(left, text)]])
    lines = []
    for _, row in rows:
        line = ""
        for left, text in sorted(row):
            col = max(int(round(left / width)), len(line) + 1 if line else 0)
            line = line.ljust(col) + text
        lines.append(line.rstrip())
    return "\n".join(lines) + "\n"


def pgm(doc, pg):
    """Page `pg` as a binary PGM at DPI (less for a page so big it would pass MAX_PIXELS): one
    copy of PDFium's bitmap, made row by row into the PGM's own buffer."""
    page = doc[pg - 1]
    try:
        w, h = page.get_size()
        scale = DPI / 72
        if w * h * scale * scale > MAX_PIXELS:
            scale = (MAX_PIXELS / max(w * h, 1)) ** 0.5 * 0.99
        bitmap = page.render(scale=scale, grayscale=True)
        try:
            width, height, stride = bitmap.width, bitmap.height, bitmap.stride
            data = memoryview(bitmap.buffer).cast("B")
            image = bytearray(b"P5\n%d %d\n255\n" % (width, height))
            if stride == width:
                image += data[:width * height]
            else:
                for y in range(height):
                    image += data[y * stride:y * stride + width]
            data.release()
        finally:
            bitmap.close()
    finally:
        page.close()
    return image


class Ocr:
    """The lock holder's OCR of `pages`: the first with eng+hin, which decides the rest's
    languages; then up to WORKERS tesseract processes at once, each killed at the deadline."""

    def __init__(self, doc, cache, deadline, say):
        self.doc, self.cache, self.deadline, self.say = doc, cache, deadline, say
        self.program, self.tessdata, _ = runtime()
        self.render_lock, self.pick_lock = threading.Lock(), threading.Lock()
        self.procs, self.done, self.failed, self.stopped = set(), {}, {}, False

    def one(self, pg, langs):
        with self.render_lock:
            if self.stopped or time.monotonic() >= self.deadline:
                return None
            image = pgm(self.doc, pg)
        if not image.startswith(b"P5") or self.stopped or time.monotonic() >= self.deadline:
            return None
        kwargs = {}
        if os.name == "nt":
            kwargs["creationflags"] = 0x4000 | 0x08000000  # BELOW_NORMAL_PRIORITY, NO_WINDOW
        import subprocess
        proc = subprocess.Popen(
            engine_command(self.program, self.tessdata, langs), stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=engine_env(), **kwargs)
        self.procs.add(proc)
        try:
            if os.name != "nt":
                try:
                    os.setpriority(os.PRIO_PROCESS, proc.pid, 10)
                except OSError:
                    pass
            out, err = proc.communicate(image, timeout=max(self.deadline - time.monotonic(), 0))
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()
            return None
        except BaseException:
            # SIGTERM (KeyboardInterrupt) or anything else mid-page: the child goes with us.
            proc.kill()
            raise
        finally:
            self.procs.discard(proc)
        del image
        if proc.returncode != 0:
            if self.stopped:
                return None
            lines = [ln for ln in err.decode("utf-8", "replace").splitlines()
                     if ln.strip() and "pixReadMemTiff" not in ln]
            reason = lines[-1] if lines else f"exit {proc.returncode}"
            self.failed[pg] = reason[:200]
            self.say(f"OCR: page {pg} failed: {reason[:200]}")
            return ""
        text = out.decode("utf-8", "replace")
        self.cache.put(pg, "ocr", text, langs)
        self.done[pg] = {"kind": "ocr", "langs": langs, "text": text}
        self.say(f"OCR: page {pg} recognised ({len(self.done)}/{self.total})")
        return text

    def run(self, pages):
        """The pages recognised, by number; a page tesseract failed on is in self.failed. Every
        tesseract this started has been killed or has exited when it returns or raises."""
        try:
            return self.recognise(pages)
        finally:
            self.stop()

    def recognise(self, pages):
        self.total, queue = len(pages), list(pages)
        probe = self.cache.meta().get("langs")
        while queue and probe is None and time.monotonic() < self.deadline:
            text = self.one(queue[0], "eng+hin")
            if text is None:
                return self.done
            queue.pop(0)
            hindi = is_devanagari(text)
            if hindi is not None:
                probe = "eng+hin" if hindi else "eng"
                self.cache.set_meta(langs=probe)
        errors = []

        def work():
            while not errors:
                with self.pick_lock:
                    if not queue or time.monotonic() >= self.deadline:
                        return
                    pg = queue.pop(0)
                try:
                    if self.one(pg, probe or "eng+hin") is None:
                        return
                except Exception as err:  # noqa: BLE001 - raised again in the main thread
                    errors.append(err)
        threads = [threading.Thread(target=work, daemon=True) for _ in range(workers())]
        for thread in threads:
            thread.start()
        for thread in threads:
            while thread.is_alive():
                thread.join(0.2)
        if errors:
            raise errors[0]
        return self.done

    def stop(self):
        self.stopped = True
        for proc in list(self.procs):
            try:
                proc.kill()
            except OSError:
                pass


def workers():
    return max(1, min((os.cpu_count() or 2) // 2, 4))


def open_doc(data, src, cache):
    import pypdfium2 as pdfium
    import pypdfium2.raw as raw

    try:
        return pdfium.PdfDocument(data)
    except pdfium.PdfiumError as err:
        reason = "encrypted" if getattr(err, "err_code", None) == raw.FPDF_ERR_PASSWORD \
            else "damaged"
        if cache:
            cache.fail(reason)
        raise Fail(OPEN_ERROR, failure(src, reason, cache is not None)) from None


def failure(src, reason, recorded, at=None):
    what = ("is encrypted (it needs a password)" if reason == "encrypted"
            else "is damaged or not a PDF")
    text = f"pdftotext: {os.path.basename(src)} {what}; its text cannot be read."
    if recorded:
        text += (f" Recorded{' on ' + at if at else ''}; Clauseo will not try this file "
                 "again until it changes.")
    return text


def ocr_root():
    root = os.environ.get("CLAUSEO_OCR_DIR", "")
    return root if root and os.path.isabs(root) else None


def pdftotext(args):
    opts, files = parse(args)
    if any(k in opts for k in ("-v", "-h", "-help", "--help", "-?")):
        print(version() + ("" if "-v" in opts else "\n" + USAGE), file=sys.stderr)
        return 0
    if not 1 <= len(files) <= 2:
        raise Fail(OTHER_ERROR, f"{version()}\n{USAGE}")
    if opts.get("-enc", "UTF-8").upper().replace("-", "") != "UTF8":
        raise Fail(OTHER_ERROR, "Command Line Error: this pdftotext writes UTF-8 only")
    quiet = opts.get("-q", False)

    def say(text):
        if not quiet:
            print(text, file=sys.stderr, flush=True)
    src = files[0]
    if len(files) == 2:
        out = files[1]
    elif src == "-":
        out = "-"
    else:
        out = (src[:-4] if src.lower().endswith(".pdf") else src) + ".txt"
    try:
        if src == "-":
            data = sys.stdin.buffer.read()
        else:
            with open(src, "rb") as file:
                data = file.read()
    except OSError as err:
        raise Fail(OPEN_ERROR, f"I/O Error: Couldn't open file '{src}': "
                   f"{err.strerror or err}.") from None
    digest = hashlib.sha256(data).hexdigest()
    program, tessdata, build = runtime()
    root = ocr_root()
    cache = Cache(root, digest, f"{build}|tesseract|{DPI}|{FORMAT}") if root else None
    if cache and cache.failed():
        record = cache.failed()
        raise Fail(OPEN_ERROR, failure(src, record.get("reason"), True, record.get("at")))
    doc = open_doc(data, src, cache)
    count = len(doc)
    first = max(opts.get("-f", 1), 1)
    last = opts.get("-l", count)
    last = count if last < 1 or last > count else last
    if first > last and count:
        raise Fail(OTHER_ERROR, f"Wrong page range given: the first page ({first}) can not be "
                   f"after the last page ({last}).")
    pages = range(first, last + 1)
    deadline = START + BUDGET
    texts, plains, scanned, unread = {}, {}, [], []
    for pg in pages:
        if pg >= first + MAX_TEXT or time.monotonic() >= deadline:
            unread = list(range(pg, last + 1))
            break
        page = doc[pg - 1]
        textpage = page.get_textpage()
        try:
            plain = textpage.get_text_range().replace("\r\n", "\n").replace("\r", "\n")
            if len(plain.strip()) >= MIN_CHARS:
                texts[pg] = layout(textpage, plain) if opts.get("-layout") else plain
                plains[pg] = plain
            else:
                scanned.append(pg)
        finally:
            textpage.close()
            page.close()
    if cache and scanned:
        # Kept only for a document with scans, for its <rel>.txt; a typed PDF leaves nothing.
        for pg, plain in plains.items():
            cache.put(pg, "text", plain)
    allowed = [pg for pg in scanned if pg < first + MAX_OCR]
    capped = [pg for pg in scanned if pg >= first + MAX_OCR]
    known = {}
    for pg in allowed:
        found = cache.page(pg) if cache else None
        if found and found.get("kind") == "ocr":
            known[pg] = found
    missing = [pg for pg in allowed if pg not in known]
    failed = {}
    unavailable = bool(missing) and not (os.path.isfile(program) and os.path.isdir(tessdata))
    if missing and not unavailable:
        say(f"OCR: {len(missing)} page(s) of {os.path.basename(src)} have no text layer; "
            "recognising them")
        while missing and time.monotonic() < deadline:
            if cache is None or cache.lock():
                try:
                    ocr = Ocr(doc, cache or Memory(), deadline, say)
                    try:
                        known.update(ocr.run(missing))
                    finally:
                        failed = ocr.failed
                    if cache:
                        cache.set_meta(engine="tesseract", dpi=DPI, pages=count)
                finally:
                    if cache:
                        cache.unlock()
                break
            time.sleep(0.25)
            for pg in missing:
                found = cache.page(pg)
                if found and found.get("kind") == "ocr":
                    known[pg] = found
            missing = [pg for pg in missing if pg not in known]
        missing = [pg for pg in allowed if pg not in known and pg not in failed]
    doc.close()
    for pg, found in known.items():
        texts[pg] = found["text"]
    name = os.path.basename(src)
    notes = []
    if missing and unavailable:
        notes.append(f"[{spans(missing)} of {name}: no text layer, and OCR is not "
                     "available: Clauseo's Python runtime has no tesseract.]")
    elif missing:
        notes.append(f"[{spans(missing)} of {name}: not yet recognised; OCR stops after "
                     f"{BUDGET:.0f} seconds per run. Run the same command again to continue, or "
                     "use -f/-l for the pages you need.]")
    for pg, reason in sorted(failed.items()):
        notes.append(f"[page {pg} of {name}: OCR failed (tesseract: {reason}); run the same "
                     "command again to retry, or check the page image.]")
    if unread:
        notes.append(f"[{spans(unread)} of {name}: not read; one run reads at most "
                     f"{MAX_TEXT:,} pages within {BUDGET:.0f} seconds. Use -f/-l for these "
                     "pages.]")
    if capped:
        notes.append(f"[{spans(capped)} of {name}: not recognised; OCR covers at most "
                     f"{MAX_OCR:,} pages per run. Use -f/-l to choose the pages, at most "
                     f"{MAX_OCR:,} at a time.]")
    brk = "" if opts.get("-nopgbrk") else "\f"
    body = "".join(texts.get(pg, "") + brk for pg in pages if pg not in unread)
    body += "".join(note + "\n" for note in notes)
    if known:
        langs = "+".join(sorted({lang for f in known.values() for lang in f["langs"].split("+")},
                                key=lambda lang: lang != "eng"))
        were = "was" if len(known) == 1 else "were"
        say(f"{spans(known)} had no text layer and {were} recognised by OCR (tesseract, "
            f"{langs}); recognition errors are possible")
    for note in notes:
        say(note)
    if cache and scanned and src != "-":
        mirror(cache, src, count, name)
    emit(body, out)
    return 0


class Memory:
    """A Cache stand-in with no folder: OCR without CLAUSEO_OCR_DIR keeps nothing."""

    def put(self, *_):
        pass

    def meta(self):
        return {}

    def set_meta(self, **_):
        pass


def mirror(cache, src, count, name):
    """<ocr>/<rel>.txt: a header line, then every page known for these bytes, \f after each;
    left as it was when END comes first."""
    path, rel = mirror_path(cache.root, src, os.getcwd())
    if path is None:
        return
    found = {}
    for pg in range(1, count + 1):
        if time.monotonic() >= START + END:
            return
        found[pg] = cache.page(pg)
    ocr = [pg for pg, f in found.items() if f and f.get("kind") == "ocr"]
    if not ocr:
        return
    langs = cache.meta().get("langs") or "eng+hin"
    todo = [pg for pg, f in found.items() if not f]
    head = (f"# OCR of {rel}, sha256 {cache.digest}, pages recognised {spans(ocr, False)} of "
            f"{count}, tesseract {langs}" + (f"; {spans(todo)} not yet read" if todo else "")
            + "\n")
    pages = "".join((f["text"] if f else f"[page {pg}: not yet read]\n") + "\f"
                    for pg, f in found.items())
    try:
        write_atomic(path, (head + pages).encode("utf-8"))
    except OSError:
        pass


def emit(body, out):
    data = body.encode("utf-8")
    if out == "-":
        sys.stdout.flush()
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
        return
    try:
        with open(out, "wb") as file:
            file.write(data)
    except OSError as err:
        raise Fail(OUTPUT_ERROR, f"I/O Error: Couldn't open text file '{out}': "
                   f"{err.strerror or err}.") from None


def utf8_text():
    """UTF-8 on stdout and stderr whatever the console's code page: `-I` ignores PYTHONUTF8 and
    PYTHONIOENCODING, and Windows' cp1252 cannot write a Hindi file name."""
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")


def main(argv):
    utf8_text()
    if not argv or argv[0] != "pdftotext":
        print(f"clauseo-pdf: unknown tool {argv[:1]!r}", file=sys.stderr)
        return OTHER_ERROR
    try:
        import signal
        signal.signal(signal.SIGTERM, signal.default_int_handler)
    except (ImportError, ValueError, OSError, AttributeError):
        pass
    try:
        return pdftotext(argv[1:])
    except Fail as fail:
        print(fail.message, file=sys.stderr)
        return fail.code
    except ImportError as err:
        print(f"Internal Error: pdftotext needs Clauseo's Python runtime ({err})", file=sys.stderr)
        return OTHER_ERROR
    except (MemoryError, ValueError, OverflowError, OSError, RuntimeError) as err:
        # PdfiumError is a RuntimeError: a page that will not load or render, in one line.
        print(f"Internal Error: pdftotext could not read the document: "
              f"{type(err).__name__}: {err}", file=sys.stderr)
        return OTHER_ERROR
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
