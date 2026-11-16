//! Prints each argument it was given on a line of its own, in brackets (`[arg]`), for the test
//! that records how Windows PowerShell 5.1 passes a word to a program
//! (`pre_write_guard/names/stetdoc/shape_tests.rs`).

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = std::io::stdout().lock();
    for arg in std::env::args_os().skip(1) {
        if writeln!(out, "[{}]", arg.to_string_lossy()).is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
