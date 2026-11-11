//! A stand-in for the stetdoc binary in `clauseo-profile`'s tests, until U13 bundles the real
//! one. It reads what to print from files beside itself, so each test copies it into a folder
//! of its own: `stub-version.txt` for `--version`, `stub-skill.md` for `skills get core`. Any
//! other arguments exit 2. While a `stub-hang` file is beside it, a run prints nothing and
//! waits a minute, then exits 6, so a test's deadline always comes first: a run past its limit
//! is killed with its tree. `stub-skill-hang` does the same to `skills get core` alone.

use std::io::Write as _;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

/// How long a run waits while `stub-hang` is there, so a stub nothing kills still exits.
const HANG_MAX: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (file, hang) = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] => ("stub-version.txt", "stub-hang"),
        ["skills", "get", "core"] => ("stub-skill.md", "stub-skill-hang"),
        _ => return ExitCode::from(2),
    };
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return ExitCode::from(3);
    };
    if dir.join("stub-hang").exists() || dir.join(hang).exists() {
        // Nothing sends: the wait ends at HANG_MAX, or when the test kills the run.
        let (_sender, never) = mpsc::channel::<()>();
        let _ended = never.recv_timeout(HANG_MAX);
        return ExitCode::from(6);
    }
    let Ok(text) = std::fs::read(dir.join(file)) else {
        return ExitCode::from(4);
    };
    if std::io::stdout().write_all(&text).is_err() {
        return ExitCode::from(5);
    }
    ExitCode::SUCCESS
}
