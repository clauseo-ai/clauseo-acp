//! `cargo xtask setup`: installs the pinned tools.

use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace;
use std::process::ExitCode;

/// A tool `cargo xtask setup` installs at a pinned version.
pub struct Tool {
    pub name: &'static str,
    pub version: &'static str,
}

pub const TOOLS: &[Tool] = &[Tool {
    name: "cargo-nextest",
    version: "0.9.148",
}];

/// Installs each tool, printing its command first. Continues past failures; fails if any failed.
pub fn run() -> ExitCode {
    let mut out = Printer::stdout();
    let root = workspace::workspace_root();
    let mut any_failed = false;
    for tool in TOOLS {
        let cmd = Cmd::new(
            Program::Cargo,
            ["install", "--locked", tool.name, "--version", tool.version],
        );
        out.note(&cmd);
        let fix = format!("run `{cmd}` to reproduce");
        match process::run_inherit(&cmd, &root) {
            Ok(true) => {}
            Ok(false) => {
                out.failure(&format!("installing {} failed", tool.name), &fix);
                any_failed = true;
            }
            Err(error) => {
                out.failure(
                    &format!("could not run cargo to install {}: {error}", tool.name),
                    &fix,
                );
                any_failed = true;
            }
        }
    }
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
