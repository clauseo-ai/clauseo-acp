//! The command line: `--scenario`, `--check`, `--list` and `--hold`, parsed by hand.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{EXIT_LOAD, EXIT_OK, EXIT_WRITE, Error, Scenario, bundled, diagnostic};

const USAGE: &str =
    "usage: fake_agent --scenario <name or path> | --check <path>... | --list | --hold";

/// Runs the command line `args` (without the program name) and returns the exit code: one of
/// [`EXIT_OK`], [`EXIT_WRITE`], [`EXIT_LOAD`] and [`crate::EXIT_MISMATCH`], or an `exit` step's
/// code. Never returns for `--hold`, or when stdin closes under `on_stdin_close: "hang"`.
pub fn cli(args: Vec<OsString>) -> i32 {
    let args: Option<Vec<String>> = args.into_iter().map(|a| a.into_string().ok()).collect();
    let Some(args) = args else {
        return fail(&diagnostic(USAGE));
    };
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--list"] => list(),
        ["--hold"] => crate::park_forever(),
        ["--scenario", target] => scenario(target),
        ["--check", paths @ ..] if !paths.is_empty() => check(paths),
        _ => fail(&diagnostic(USAGE)),
    }
}

/// Writes `text` to stderr and returns [`EXIT_LOAD`].
fn fail(text: &str) -> i32 {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(text.as_bytes()).unwrap_or_default();
    EXIT_LOAD
}

/// Writes `text` to stdout.
fn print(text: &str) -> i32 {
    print_to(&mut std::io::stdout().lock(), text)
}

/// Writes `text` to `out` and returns [`EXIT_OK`], or [`EXIT_WRITE`] with a stderr line if
/// the write fails (the client closed stdout).
fn print_to(out: &mut impl Write, text: &str) -> i32 {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => EXIT_OK,
        Err(error) => {
            let mut stderr = std::io::stderr().lock();
            let line = diagnostic(format_args!("write to stdout failed: {error}"));
            stderr.write_all(line.as_bytes()).unwrap_or_default();
            EXIT_WRITE
        }
    }
}

fn list() -> i32 {
    let mut text = String::new();
    for name in bundled::SCENARIOS {
        text.push_str(name);
        text.push('\n');
    }
    print(&text)
}

/// A bare name is a bundled scenario; anything with a path separator or `.jsonl` is a file.
fn scenario(target: &str) -> i32 {
    let loaded = if target.contains(['/', '\\'])
        || Path::new(target)
            .extension()
            .is_some_and(|ext| ext == "jsonl")
    {
        Scenario::load(Path::new(target))
    } else {
        match bundled::text(target) {
            Some(text) => Scenario::parse(target, text),
            None => {
                return fail(&diagnostic(format_args!(
                    "unknown scenario `{target}`; `fake_agent --list` prints the bundled names"
                )));
            }
        }
    };
    match loaded {
        Ok(scenario) => crate::run::run(&scenario),
        Err(error) => fail(&diagnostic(error)),
    }
}

/// Loads every file named, and every `.jsonl` file directly in each folder named.
fn check(paths: &[&str]) -> i32 {
    let mut errors = Vec::new();
    let mut count = 0_usize;
    for path in paths {
        match files(Path::new(path)) {
            Ok(files) => {
                for file in files {
                    match Scenario::load(&file) {
                        Ok(_) => count += 1,
                        Err(error) => errors.push(diagnostic(error)),
                    }
                }
            }
            Err(error) => errors.push(diagnostic(error)),
        }
    }
    if errors.is_empty() {
        print(&format!("ok {count}\n"))
    } else {
        fail(&errors.concat())
    }
}

/// The scenario files `path` names: itself, or the `.jsonl` files directly in it, sorted. A
/// folder with none is an error, so a check that finds nothing never passes.
fn files(path: &Path) -> Result<Vec<PathBuf>, Error> {
    let io = |source| Error::Io {
        path: path.to_owned(),
        source,
    };
    if !path.is_dir() {
        return if path.is_file() {
            Ok(vec![path.to_owned()])
        } else {
            Err(io(std::io::Error::from(std::io::ErrorKind::NotFound)))
        };
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path).map_err(io)? {
        let file = entry.map_err(io)?.path();
        if file.is_file() && file.extension().is_some_and(|ext| ext == "jsonl") {
            files.push(file);
        }
    }
    if files.is_empty() {
        return Err(io(std::io::Error::other("no .jsonl files in this folder")));
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stdout the client has closed: every write fails.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_stdout_write_exits_write_not_mismatch() {
        assert_eq!(print_to(&mut Closed, "ok 1\n"), EXIT_WRITE);
        let mut out = Vec::new();
        assert_eq!(print_to(&mut out, "ok 1\n"), EXIT_OK);
        assert_eq!(out, b"ok 1\n");
    }
}
