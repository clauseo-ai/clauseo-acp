//! The command line: `--scenario`, `--check`, `--list` and `--hold`, parsed by hand.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{Error, Scenario, bundled};

const USAGE: &str = concat!(
    "fake_agent: usage: fake_agent --scenario <name or path> | --check <path>... | --list | ",
    "--hold\n"
);

/// Runs the command line `args` (without the program name) and returns the exit code: 0 done,
/// 2 bad arguments or a scenario that fails to load, 3 a mismatch, or an `exit` step's code.
/// Never returns for `--hold`, or when stdin closes under `on_stdin_close: "hang"`.
pub fn cli(args: Vec<OsString>) -> i32 {
    let args: Option<Vec<String>> = args.into_iter().map(|a| a.into_string().ok()).collect();
    let Some(args) = args else {
        return fail(USAGE);
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
        _ => fail(USAGE),
    }
}

/// Writes `text` to stderr and returns 2.
fn fail(text: &str) -> i32 {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(text.as_bytes()).unwrap_or_default();
    2
}

/// Writes `text` to stdout; exits 3 if the client closed it.
fn print(text: &str) -> i32 {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(error) => {
            let mut stderr = std::io::stderr().lock();
            let line = format!("fake_agent: write to stdout failed: {error}\n");
            stderr.write_all(line.as_bytes()).unwrap_or_default();
            3
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
                return fail(&format!(
                    "fake_agent: unknown scenario `{target}`; `fake_agent --list` prints the \
                     bundled names\n"
                ));
            }
        }
    };
    match loaded {
        Ok(scenario) => crate::run::run(&scenario),
        Err(error) => fail(&format!("fake_agent: {error}\n")),
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
                        Err(error) => errors.push(format!("fake_agent: {error}\n")),
                    }
                }
            }
            Err(error) => errors.push(format!("fake_agent: {error}\n")),
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
