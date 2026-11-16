//! A stand-in for stetdoc in the Word blackline's host tests (`tests/host/blackline.rs`). Each
//! test copies it into a folder of its own and puts what it should do beside it:
//!
//! - `compare`: copies its two inputs beside itself as `seen-old.docx` and `seen-new.docx`,
//!   writes its arguments, one per line, to `args-compare.txt`, writes the `-o` file, and prints
//!   `compare.json` from beside itself (stetdoc's answer, its receipt's warnings with it), or,
//!   with none there, a receipt with no warnings; with `compare-exit.txt` beside it, exits with
//!   the code in it instead, first printing `compare-out.json` from beside itself when there is
//!   one (stetdoc's JSON error).
//! - `view` of the redline: writes its arguments to `args-view.txt` and prints `view.json` from
//!   beside itself; `view-mode.txt` may say `hang` (never answers), `big` (prints 9 MiB) or
//!   `exit <code>`.
//! - `view --story notes` or `--story headers` of the redline: writes its arguments to
//!   `args-view-story-notes.txt` or `args-view-story-headers.txt` and prints
//!   `view-story-notes.json` or `view-story-headers.json` from beside itself, or, with none
//!   there, a view of an empty story; `view-story-mode.txt` may say `exit <code>`.
//! - `view` of a version (`old.docx`, `new.docx`): writes its arguments to `args-view-old.txt`
//!   or `args-view-new.txt` and prints `view-old.json` or `view-new.json` from beside itself, or,
//!   with none there, a view of an empty document.
//! - `check`: writes its arguments to `args-check.txt` and prints `check.json` from beside
//!   itself; with none there, exits 2 (as a stetdoc that can't check).
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
        Some("check") => check(&dir, &args),
        _ => return ExitCode::from(2),
    };
    done.unwrap_or(ExitCode::from(3))
}

/// The `compare` run: `compare --json --author=A -o OUT OLD NEW`.
fn compare(dir: &Path, args: &[String]) -> Option<ExitCode> {
    std::fs::write(dir.join("args-compare.txt"), args.join("\n")).ok()?;
    if let Ok(code) = std::fs::read_to_string(dir.join("compare-exit.txt")) {
        if let Ok(out) = std::fs::read(dir.join("compare-out.json")) {
            std::io::stdout().write_all(&out).ok()?;
        }
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
    let answer = std::fs::read(dir.join("compare.json"))
        .unwrap_or_else(|_none| NO_WARNINGS.as_bytes().to_vec());
    std::io::stdout().write_all(&answer).ok()?;
    Some(ExitCode::SUCCESS)
}

/// stetdoc's answer to a compare that warns of nothing: the fields the host reads.
const NO_WARNINGS: &str = r#"{"output":"redline.docx","receipt":{"warnings":[]}}"#;

/// A story view of the redline with nothing in it, as stetdoc 0.9.3 prints one; `STORY` stands
/// for the story's name.
const EMPTY_STORY: &str = concat!(
    r#"{"mode":"redline","next":null,"position":{"shown":0,"total":0},"warnings":[],"#,
    r#""text":"{stet 1 view=STORY mode=redline}\n{summary: 1 section}\n"}"#
);

/// A view of an empty document, for a version with no view of its own beside the stand-in.
const EMPTY_VIEW: &str = r#"{"mode":"final","next":null,"text":"{stet 1 view=body mode=final}\n"}"#;

/// The `check` run: `check --json --author=A REDLINE`.
fn check(dir: &Path, args: &[String]) -> Option<ExitCode> {
    std::fs::write(dir.join("args-check.txt"), args.join("\n")).ok()?;
    let Ok(out) = std::fs::read(dir.join("check.json")) else {
        return Some(ExitCode::from(2));
    };
    std::io::stdout().write_all(&out).ok()?;
    Some(ExitCode::SUCCESS)
}

/// The `view` run: `view --all --json REDLINE`, or `view --all --json --mode final VERSION`.
fn view(dir: &Path, args: &[String]) -> Option<ExitCode> {
    let input = Path::new(args.last()?).file_name()?.to_str()?;
    if let Some(side) = input
        .strip_suffix(".docx")
        .filter(|s| ["old", "new"].contains(s))
    {
        std::fs::write(dir.join(format!("args-view-{side}.txt")), args.join("\n")).ok()?;
        let out = std::fs::read(dir.join(format!("view-{side}.json")))
            .unwrap_or_else(|_none| EMPTY_VIEW.as_bytes().to_vec());
        std::io::stdout().write_all(&out).ok()?;
        return Some(ExitCode::SUCCESS);
    }
    if let Some(story) = args
        .iter()
        .position(|arg| arg == "--story")
        .and_then(|at| args.get(at + 1))
    {
        return view_story(dir, args, story);
    }
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

/// The `view --story <story>` run on the redline.
fn view_story(dir: &Path, args: &[String], story: &str) -> Option<ExitCode> {
    std::fs::write(
        dir.join(format!("args-view-story-{story}.txt")),
        args.join("\n"),
    )
    .ok()?;
    let mode = std::fs::read_to_string(dir.join("view-story-mode.txt")).unwrap_or_default();
    if let Some(code) = mode.trim().strip_prefix("exit ") {
        return Some(ExitCode::from(code.parse::<u8>().ok()?));
    }
    let out = std::fs::read(dir.join(format!("view-story-{story}.json")))
        .unwrap_or_else(|_none| EMPTY_STORY.replace("STORY", story).into_bytes());
    std::io::stdout().write_all(&out).ok()?;
    Some(ExitCode::SUCCESS)
}
