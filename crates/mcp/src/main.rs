//! The `clauseo-mcp` binary: picks a mode by its first argument. Every mode lives in the library.

use std::ffi::OsString;

use clauseo_mcp::{Failure, browser_guard};

/// A mode's entry point: the arguments after the mode's name, and the exit code or a failure.
type Run = fn(&[OsString]) -> Result<i32, Failure>;

/// Every mode, by the exact name argv[1] must hold. The usage line is built from this table.
const MODES: &[(&str, Run)] = &[("browser-guard", browser_guard::run)];

/// The exit code for a command line no mode takes.
const EXIT_USAGE: i32 = 2;

fn usage() -> String {
    let names: Vec<&str> = MODES.iter().map(|(name, _)| *name).collect();
    format!(
        "usage: clauseo-mcp <mode> ... (modes: {})",
        names.join(", ")
    )
}

#[expect(
    clippy::print_stderr,
    reason = "main prints a mode's failure on stderr; stdout carries the protocol"
)]
fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mode = args
        .first()
        .and_then(|name| MODES.iter().find(|(mode, _)| name.as_os_str() == *mode));
    let result = match mode {
        Some((_, run)) => run(args.get(1..).unwrap_or_default()),
        None => Err(Failure {
            code: EXIT_USAGE,
            message: usage(),
        }),
    };
    let code = match result {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("{}", failure.message);
            failure.code
        }
    };
    std::process::exit(code);
}
