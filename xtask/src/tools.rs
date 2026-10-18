//! `cargo xtask setup`: installs the pinned tools.

use std::process::{Command, ExitCode};

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
pub fn setup() -> ExitCode {
    let root = crate::workspace::workspace_root();
    let mut any_failed = false;
    for tool in TOOLS {
        println!(
            "cargo install --locked {} --version {}",
            tool.name, tool.version
        );
        let result = Command::new("cargo")
            .args(["install", "--locked", tool.name, "--version", tool.version])
            .current_dir(&root)
            .status();
        match result {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("error: installing {} failed ({status})", tool.name);
                any_failed = true;
            }
            Err(error) => {
                eprintln!(
                    "error: could not run cargo to install {}: {error}",
                    tool.name
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
