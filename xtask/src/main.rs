//! Workspace automation: `cargo xtask check [--quick]` and `cargo xtask setup`.
//!
//! For people and agents:
//! - Add an external step: one `ToolStep` in `steps/plan.rs`. Its `parse` turns the output into findings.
//! - Add a static check: a new file in `src/checks/` with a `StaticCheck`, plus one line in `checks::ALL`.
//! - Pin a tool: one `Tool` entry in `setup::TOOLS`.
//! - Failures print as `path:line: what`, then `    fix: ...`. A failure with no location prints the tool's output once, then a `fix:` line.
//! - Live tool output appears only when stdout is a terminal. Otherwise a passing tool prints its header and our findings only.
//! - Only `process.rs` starts processes and only `output.rs` prints. Keep it that way.

mod changes;
mod check;
mod checks;
mod output;
mod parse;
mod process;
mod setup;
mod steps;
mod violation;
mod workspace;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

/// Workspace automation for Clauseo.
#[derive(Parser)]
#[command(name = "xtask", bin_name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run fmt, the static checks, clippy and nextest. Failures print as `path:line: what` and `fix:`.
    Check {
        /// Only check packages changed since `git merge-base HEAD origin/main`, and their dependents.
        #[arg(long)]
        quick: bool,
    },
    /// Install the pinned tools with `cargo install --locked <tool> --version <version>`.
    Setup,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Commands::Check { quick } => check::run(quick),
        Commands::Setup => setup::run(),
    }
}
