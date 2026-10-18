//! Workspace automation: `cargo xtask check [--quick]` and `cargo xtask setup`.

mod changes;
mod check;
mod parse;
mod steps;
mod tools;
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
        Commands::Setup => tools::setup(),
    }
}
