//! A stand-in for the stetdoc program in the host's tracked-changes tests
//! (`tests/host/revisions.rs`). The host starts it with a clean environment, so it reads what to
//! do from files beside itself, which each test writes into a folder of its own:
//!
//! - `check.json` and `view.json`: what `check` and `view` print.
//! - `edit.txt`: how `edit` answers. Missing or `ok`: it writes `-o`'s file as the input's bytes
//!   followed by the `--ops` batch, and prints `{}`. `huge`: as `ok`, with the file grown
//!   (sparse) past the matters' size cap, so the host fails to store it. `hang`: it never ends.
//!   `garbage`: it prints text that is not JSON. `<code> <exit>`: it prints stetdoc's JSON
//!   refusal with `code` and exits with `exit`.
//!
//! Every run appends its first argument to `calls.txt`, so a test can tell whether `edit` ran.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return ExitCode::from(90);
    };
    let command = args.first().map_or("", String::as_str);
    let mut calls = std::fs::read(dir.join("calls.txt")).unwrap_or_default();
    calls.extend_from_slice(format!("{command}\n").as_bytes());
    if write(&dir.join("calls.txt"), &calls, None).is_err() {
        return ExitCode::from(91);
    }
    match command {
        "check" => print(&dir.join("check.json")),
        "view" => print(&dir.join("view.json")),
        "edit" => edit(&dir, &args),
        _ => ExitCode::from(2),
    }
}

/// Prints the file `path`.
fn print(path: &Path) -> ExitCode {
    let Ok(text) = std::fs::read(path) else {
        return ExitCode::from(92);
    };
    if std::io::stdout().write_all(&text).is_err() {
        return ExitCode::from(93);
    }
    ExitCode::SUCCESS
}

/// `edit <input> --ops <batch> -o <out> ...`, as `edit.txt` says.
fn edit(dir: &Path, args: &[String]) -> ExitCode {
    let mode = std::fs::read_to_string(dir.join("edit.txt")).unwrap_or_else(|_| "ok".to_owned());
    let mode = mode.trim();
    let mut len = None;
    match mode {
        "ok" => {}
        "huge" => len = Some(HUGE),
        "hang" => hang(),
        "garbage" => {
            return say("this is not JSON\n", ExitCode::SUCCESS);
        }
        refusal => {
            let (code, exit) = refusal.split_once(' ').unwrap_or((refusal, "1"));
            let text = format!("{{\"error\":{{\"code\":\"{code}\",\"message\":\"refused\"}}}}\n");
            return say(&text, ExitCode::from(exit.parse::<u8>().unwrap_or(1)));
        }
    }
    let after = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|at| args.get(at + 1))
            .map(PathBuf::from)
    };
    let (Some(input), Some(ops), Some(out)) = (args.get(1), after("--ops"), after("-o")) else {
        return ExitCode::from(94);
    };
    let (Ok(mut bytes), Ok(batch)) = (std::fs::read(input), std::fs::read(ops)) else {
        return ExitCode::from(95);
    };
    bytes.extend_from_slice(&batch);
    if write(&out, &bytes, len).is_err() {
        return ExitCode::from(96);
    }
    say("{}\n", ExitCode::SUCCESS)
}

/// Never ends: stetdoc stuck, for the host's deadline to kill.
fn hang() -> ! {
    loop {
        std::thread::park();
    }
}

/// Prints `text`, then ends with `code`.
fn say(text: &str, code: ExitCode) -> ExitCode {
    if std::io::stdout().write_all(text.as_bytes()).is_err() {
        return ExitCode::from(93);
    }
    code
}

/// One byte past the matters' size cap (`clauseo_matters::FILE_CAP_BYTES`).
const HUGE: u64 = 512_000_001;

/// Writes `bytes` to `path`, as stetdoc writes its output, then grows the file to `len` bytes
/// if given.
#[expect(
    clippy::disallowed_methods,
    reason = "a stand-in for stetdoc, which writes its -o file and its own call log"
)]
fn write(path: &Path, bytes: &[u8], len: Option<u64>) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    if let Some(len) = len {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(len)?;
    }
    Ok(())
}
