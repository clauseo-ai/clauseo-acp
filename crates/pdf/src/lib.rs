//! Clauseo's own `pdftoppm` and `pdfinfo` (scanned PDFs, PR-P1) and `pdftotext` (PR-O2): small
//! programs that run Clauseo's Python runtime on [`SCRIPT`], which renders pages and reads page
//! counts with the runtime's `pypdfium2` (`PDFium`, BSD/Apache) and Pillow, or on
//! [`PDFTOTEXT`], which prints a PDF's text layer and recognises pages without one with the
//! runtime's tesseract. No poppler (GPL) code ships.
//!
//! Claude Code's Read renders a long PDF by running `pdftoppm -v` (it runs: the tool is there),
//! then `pdftoppm -jpeg -r 100 [-f N] [-l M] <pdf> <prefix>`, and counts pages with
//! `pdfinfo <pdf>`'s `Pages:` line, each by name on its `PATH`, with no shell; on Windows that
//! needs a real `.exe`, hence these programs rather than a script. Codex has no PDF input and is
//! told to run `pdftoppm -png` itself, then view the images.
//!
//! Each program finds the interpreter ([`find_python`]) only one way, with no variable to
//! override it: the first folder on `PATH` that is, or is inside, a Clauseo Python runtime
//! folder (it holds [`RECEIPT`]): the desktop puts the runtime's folders on the agents' `PATH`
//! with this tool's folder, and only once the runtime is installed. It runs it isolated (`-I`:
//! no `PYTHON*` variable, no user site, no current folder on the module path), in UTF-8 mode,
//! with the script as `-c` (so the bundle holds no loose file beside the programs), the tool's
//! name and the arguments as given. With no runtime it exits [`NOT_FOUND`], as a shell does for a
//! missing command, so Claude Code's check finds no `pdftoppm` and says so.
//!
//! On Unix the program becomes the interpreter (`exec`): one process, so a signal sent to it
//! (Claude Code's SIGTERM at its timeout) reaches Python itself. On Windows it waits for it in
//! a Job Object that ends it when the launcher is killed (`job.rs`), and exits with its code.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[cfg(windows)]
mod job;

/// The Python script `pdftoppm` and `pdfinfo` run, over `pypdfium2` and Pillow.
pub const SCRIPT: &str = include_str!("clauseo_pdf.py");

/// The Python script `pdftotext` runs: the text layer over `pypdfium2`, the other pages OCR'd
/// by the runtime's tesseract, kept in the matter's notes (`CLAUSEO_OCR_DIR`).
pub const PDFTOTEXT: &str = include_str!("pdftotext.py");

/// The receipt every installed Clauseo Python runtime folder holds (`clauseo_agents`'
/// `python.rs`, `PYTHON_RECEIPT`).
pub const RECEIPT: &str = "clauseo-python.json";

/// The exit code with no interpreter, or one that would not start: a shell's "not found".
pub const NOT_FOUND: u8 = 127;

/// The variables taken out of the interpreter's environment (`-I` ignores them too).
const DROPPED: [&str; 2] = ["PYTHONHOME", "PYTHONPATH"];

/// The poppler tool a program stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `pdftoppm`: pages to images.
    Pdftoppm,
    /// `pdfinfo`: the page count and the document's details.
    Pdfinfo,
    /// `pdftotext`: the text, with pages that have no text layer recognised by OCR.
    Pdftotext,
}

impl Tool {
    /// The tool's command name, which the script dispatches on.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pdftoppm => "pdftoppm",
            Self::Pdfinfo => "pdfinfo",
            Self::Pdftotext => "pdftotext",
        }
    }

    /// The Python script the tool runs.
    pub const fn script(self) -> &'static str {
        match self {
            Self::Pdftoppm | Self::Pdfinfo => SCRIPT,
            Self::Pdftotext => PDFTOTEXT,
        }
    }
}

/// How a runtime folder lays out its interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// `python.exe` in the runtime's root.
    Windows,
    /// `bin/python3`.
    Unix,
}

impl Os {
    /// This program's OS.
    pub const fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// The interpreter inside the runtime folder `root` on `os`.
pub fn runtime_program(root: &Path, os: Os) -> PathBuf {
    match os {
        Os::Windows => root.join("python.exe"),
        Os::Unix => root.join("bin").join("python3"),
    }
}

/// The interpreter to run: along `path` (the `PATH` value) in order, the first absolute folder
/// that holds [`RECEIPT`] or whose parent does (macOS `<rt>/bin`, Windows `<rt>` and
/// `<rt>\clauseo-bin`), and whose runtime has its interpreter. `None` when there is none.
pub fn find_python(path: Option<&OsStr>, os: Os) -> Option<PathBuf> {
    find_python_with(path, os, &|file| file.is_file())
}

/// [`find_python`], with `is_file` saying whether a file is there (the tests' file system).
#[expect(
    clippy::disallowed_methods,
    reason = "the launcher walks PATH once to find the Clauseo Python runtime the desktop put on it"
)]
pub fn find_python_with(
    path: Option<&OsStr>,
    os: Os,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| {
            let roots = [Some(dir.as_path()), dir.parent()];
            roots.into_iter().flatten().find_map(|root| {
                let program = runtime_program(root, os);
                (is_file(&root.join(RECEIPT)) && is_file(&program)).then_some(program)
            })
        })
}

/// The interpreter's command for `tool` with `args`: `python -I -X utf8 -c <script> <tool>
/// <args…>`, without [`DROPPED`] in its environment.
pub fn command(python: &Path, tool: Tool, args: &[OsString]) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-I", "-X", "utf8", "-c", tool.script(), tool.name()])
        .args(args);
    for var in DROPPED {
        command.env_remove(var);
    }
    command
}

/// A program's whole run: finds the interpreter from this process's environment and runs
/// [`command`] with its own arguments, or says why not and exits [`NOT_FOUND`].
pub fn run(tool: Tool) -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let path = std::env::var_os("PATH");
    let Some(python) = find_python(path.as_deref(), Os::host()) else {
        say(&format!(
            "{}: Clauseo's Python runtime was not found; it is set up from Clauseo's settings",
            tool.name()
        ));
        return ExitCode::from(NOT_FOUND);
    };
    start(tool, &mut command(&python, tool, &args))
}

/// Becomes the interpreter; returns only when it could not start.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the launcher's one job is to become the interpreter; the process crate is async"
)]
fn start(tool: Tool, command: &mut Command) -> ExitCode {
    use std::os::unix::process::CommandExt as _;
    let error = command.exec();
    say(&format!(
        "{}: Clauseo's Python runtime did not start: {error}",
        tool.name()
    ));
    ExitCode::from(NOT_FOUND)
}

/// Runs the interpreter in a kill-on-close job (`job.rs`), held until it exits, and exits with
/// its code ([`exit_byte`]). A job that cannot be made or joined is said on stderr; the
/// interpreter still runs.
#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "the launcher's one job is to run the interpreter and pass on its code"
)]
fn start(tool: Tool, command: &mut Command) -> ExitCode {
    let job = job::Job::new();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            say(&format!(
                "{}: Clauseo's Python runtime did not start: {error}",
                tool.name()
            ));
            return ExitCode::from(NOT_FOUND);
        }
    };
    let joined = match &job {
        Ok(job) => job.assign(&child),
        Err(error) => Err(std::io::Error::other(error.to_string())),
    };
    if let Err(error) = joined {
        say(&format!(
            "{}: Python runs outside a job: {error}",
            tool.name()
        ));
    }
    let status = child.wait();
    // The job's handle closes only now, after the interpreter has exited.
    drop(job);
    match status {
        Ok(status) => ExitCode::from(exit_byte(status.code())),
        Err(error) => {
            say(&format!(
                "{}: Python's exit was not read: {error}",
                tool.name()
            ));
            ExitCode::from(99)
        }
    }
}

/// The program's exit code for the interpreter's `code`: the same when it fits in a byte, else
/// 99, poppler's "other error".
pub fn exit_byte(code: Option<i32>) -> u8 {
    code.and_then(|code| u8::try_from(code).ok()).unwrap_or(99)
}

/// One line on stderr; a failed write is dropped (there is nowhere else to say it).
fn say(text: &str) {
    writeln!(std::io::stderr().lock(), "{text}").unwrap_or_default();
}

#[cfg(test)]
mod tests;
