//! A stand-in for stetdoc in the Word blackline's host tests (`tests/host/blackline.rs`). Each
//! test copies it into a folder of its own and puts what it should do beside it:
//!
//! - `compare`: copies its two inputs beside itself as `seen-old.docx` and `seen-new.docx`,
//!   writes its arguments, one per line, to `args-compare.txt`, writes the `-o` file, and prints
//!   `{}`; with `compare-exit.txt` beside it, exits with the code in it instead.
//! - `view`: writes its arguments to `args-view.txt` and prints `view.json` from beside itself;
//!   `view-mode.txt` may say `hang` (never answers), `big` (prints 9 MiB) or `exit <code>`.
//!
//! Any other first argument exits 2; a file it needs and can't read exits 3.

#![expect(
    clippy::disallowed_methods,
    reason = "a test's stand-in for stetdoc, a program of its own: it writes the files stetdoc \
              would and what it was given, beside itself in the test's folder"
)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return ExitCode::from(3);
    };
    let done = match args.first().map(String::as_str) {
        Some("compare") => compare(&dir, &args),
        Some("view") => view(&dir, &args),
        _ => return ExitCode::from(2),
    };
    done.unwrap_or(ExitCode::from(3))
}

/// The `compare` run: `compare --json --author=A -o OUT OLD NEW`.
fn compare(dir: &Path, args: &[String]) -> Option<ExitCode> {
    std::fs::write(dir.join("args-compare.txt"), args.join("\n")).ok()?;
    if let Ok(code) = std::fs::read_to_string(dir.join("compare-exit.txt")) {
        return Some(ExitCode::from(code.trim().parse::<u8>().ok()?));
    }
    let [.., out, old, new] = args else {
        return None;
    };
    let old = std::fs::read(old).ok()?;
    let new = std::fs::read(new).ok()?;
    std::fs::write(dir.join("seen-old.docx"), &old).ok()?;
    std::fs::write(dir.join("seen-new.docx"), &new).ok()?;
    std::fs::write(PathBuf::from(out), &new).ok()?;
    std::io::stdout().write_all(b"{}").ok()?;
    Some(ExitCode::SUCCESS)
}

/// The `view` run: `view --all --json REDLINE`.
fn view(dir: &Path, args: &[String]) -> Option<ExitCode> {
    std::fs::write(dir.join("args-view.txt"), args.join("\n")).ok()?;
    let mode = std::fs::read_to_string(dir.join("view-mode.txt")).unwrap_or_default();
    let mode = mode.trim();
    let mut stdout = std::io::stdout();
    if mode == "hang" {
        // Never answers: the sender stays alive, so nothing ever arrives.
        let (_keep, wait) = std::sync::mpsc::channel::<()>();
        let _never = wait.recv();
        return Some(ExitCode::from(4));
    }
    if mode == "big" {
        let line = [b'x'; 1024];
        for _ in 0..9 * 1024 {
            stdout.write_all(&line).ok()?;
        }
        return Some(ExitCode::SUCCESS);
    }
    if let Some(code) = mode.strip_prefix("exit ") {
        return Some(ExitCode::from(code.parse::<u8>().ok()?));
    }
    stdout
        .write_all(&std::fs::read(dir.join("view.json")).ok()?)
        .ok()?;
    Some(ExitCode::SUCCESS)
}
