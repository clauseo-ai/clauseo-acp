//! Test only, never shipped (sidecars.mjs bundles only `PDF_TOOLS`): a fake `tesseract.exe` for
//! `tests/test_pdftotext.py` on Windows, which runs no `#!python` script as a program. It does
//! what that file's Python fake does: checks the O2 rules (`stdin stdout -l <langs>
//! --tessdata-dir <dir>`, no `TESSDATA_PREFIX`, `OMP_THREAD_LIMIT=1`, a PGM on stdin), appends
//! its pid to `FAKE_PIDS`, writes leptonica's chatter on stderr, fails on a page `FAKE_FAIL_WIDTH`
//! wide, sleeps `FAKE_SLEEP` seconds, logs `<langs> <width>` to `FAKE_LOG` and prints words
//! naming the width (Hindi ones with `FAKE_HINDI=1` and `hin` asked for), then a form feed with
//! `FAKE_FF` set, as the real one ends its page.
#![expect(
    clippy::disallowed_methods,
    reason = "a test fake: it appends to the files its test names and sleeps as told"
)]

use std::io::{Read as _, Write as _};
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut data = Vec::new();
    if std::io::stdin().read_to_end(&mut data).is_err() {
        return ExitCode::from(3);
    }
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    let ok = args.len() == 6
        && args[..2] == ["stdin", "stdout"]
        && args[2] == "-l"
        && args[4] == "--tessdata-dir"
        && std::env::var_os("TESSDATA_PREFIX").is_none()
        && data.starts_with(b"P5\n")
        && var("OMP_THREAD_LIMIT") == "1";
    let mut err = std::io::stderr().lock();
    if !ok {
        writeln!(err, "broken rules: {args:?}").unwrap_or_default();
        return ExitCode::from(3);
    }
    let width = String::from_utf8_lossy(&data)
        .split_ascii_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let append = |path: String, line: &str| {
        if !path.is_empty() {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path);
            if let Ok(mut file) = file {
                file.write_all(line.as_bytes()).unwrap_or_default();
            }
        }
    };
    append(var("FAKE_PIDS"), &format!("{}\n", std::process::id()));
    write!(
        err,
        "Error in pixReadMemTiff: function not present\nError in pixReadMem: tiff: no pix \
         returned\nEstimating resolution as 300\n"
    )
    .unwrap_or_default();
    if width == var("FAKE_FAIL_WIDTH") {
        write!(
            err,
            "Error: page broke\nError in pixaGenerateFont: pixa not made\n"
        )
        .unwrap_or_default();
        return ExitCode::from(1);
    }
    let sleep: f64 = var("FAKE_SLEEP").parse().unwrap_or(0.0);
    std::thread::sleep(Duration::from_secs_f64(sleep.max(0.0)));
    let langs = &args[3];
    append(var("FAKE_LOG"), &format!("{langs} {width}\n"));
    let text = if var("FAKE_HINDI") == "1" && langs.contains("hin") {
        "\u{928}\u{94d}\u{92f}\u{93e}\u{92f}\u{93e}\u{932}\u{92f} ".repeat(5)
    } else {
        "recognised words ".to_owned()
    };
    let mut out = std::io::stdout().lock();
    let page_break = if var("FAKE_FF").is_empty() {
        ""
    } else {
        "\x0c"
    };
    if writeln!(out, "{text}w{width}").is_err() || write!(out, "{page_break}").is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
