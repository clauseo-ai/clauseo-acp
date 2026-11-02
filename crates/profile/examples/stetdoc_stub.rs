//! A stand-in for the stetdoc binary in `clauseo-profile`'s tests, until U13 bundles the real
//! one. It reads what to print from files beside itself, so each test copies it into a folder
//! of its own: `stub-version.txt` for `--version`, `stub-skill.md` for `skills get core`. Any
//! other arguments exit 2.

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let file = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] => "stub-version.txt",
        ["skills", "get", "core"] => "stub-skill.md",
        _ => return ExitCode::from(2),
    };
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return ExitCode::from(3);
    };
    let Ok(text) = std::fs::read(dir.join(file)) else {
        return ExitCode::from(4);
    };
    if std::io::stdout().write_all(&text).is_err() {
        return ExitCode::from(5);
    }
    ExitCode::SUCCESS
}
