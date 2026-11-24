//! Checks codex-acp's network patch against an install of a pair of pins, for the engine watch.
//!
//! `cargo run -p clauseo-agents --example codex_patch_check -- <node_modules>`
//!
//! Run it on the `node_modules` of an `npm ci --ignore-scripts` of the pair. Prints `ok: <file>`
//! and exits 0 when each mode's pattern matches exactly once and the patched file passes
//! `node --check`. Otherwise it prints `codex_patch_check: <mode or step>: <file>: <reason>` to
//! stderr and exits 1. A missing or extra argument exits 2.

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use clauseo_agents::check_codex_patch;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(node_modules), None) = (args.next(), args.next()) else {
        // Nothing is left to do when stderr is gone too.
        drop(writeln!(
            io::stderr().lock(),
            "usage: codex_patch_check <node_modules>"
        ));
        return ExitCode::from(2);
    };
    match check_codex_patch(Path::new(&node_modules)) {
        Ok(file) => match writeln!(io::stdout().lock(), "ok: {}", file.display()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(err) => {
            // Nothing is left to do when stderr is gone too.
            drop(writeln!(
                io::stderr().lock(),
                "codex_patch_check: {}: {}: {err}",
                err.step(),
                err.file().display()
            ));
            ExitCode::FAILURE
        }
    }
}
