//! Workspace automation: `cargo xtask check [--quick]`, `cargo xtask setup [--binaries]` and
//! `cargo xtask buildtime [rust|web|all] [--runs N] [--json]`.
//!
//! For people and agents:
//! - Add an external step: one `ToolStep` in `steps/plan.rs`, or `steps/policy.rs` for a
//!   repo-policy tool. Its `parse` turns the output into findings.
//! - Add a static check: a new file in `src/checks/` with a `StaticCheck`, plus one line in
//!   `checks::ALL`. Its `run` returns an `Inspection` that counts what it inspected (zero fails),
//!   and its planted cases go in one `checks::planted::cases!` list.
//! - Pin a tool: one `Tool` in `setup::TOOLS` for a cargo crate, else one `Binary` in
//!   `setup::BINARIES`.
//! - `setup` prints a `Binary`'s install command; `setup --binaries` also runs it.
//! - Budgets live in `budgets.toml` at the repo root; `budgets.rs` reads them for the check and for
//!   `buildtime`.
//! - Failures print as `path:line: what`, then `    fix: ...`. A failure with no location prints
//!   the tool's output once, then a `fix:` line.
//! - Live tool output appears only when stdout is a terminal. Otherwise a passing tool prints its
//!   header and our findings only.
//! - Only `process.rs` starts processes and only `output.rs` prints. Keep it that way. The one
//!   clock read (`clock.rs`) and each call to a disallowed method (`Command::spawn`,
//!   `Command::output`) carry
//!   `#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]`.
//! - `buildtime web` runs `node ui/app/scripts/buildtime.mjs`, which times the
//!   `ui/app/package.json` scripts with Node's own clock and prints JSON (the contract is in
//!   `buildtime/web.rs`); xtask computes the statistics and judges them against budgets.

mod budgets;
mod buildtime;
mod changes;
mod check;
mod checks;
mod clock;
mod fix;
mod output;
mod parse;
mod process;
#[cfg(test)]
mod scratch;
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
    /// Run fmt, the static checks, clippy, the ui steps and nextest. Failures print as
    /// `path:line: what` and `fix:`.
    Check {
        /// Only check packages changed since `git merge-base HEAD origin/main`, and their
        /// dependents.
        #[arg(long)]
        quick: bool,
    },
    /// Install the pinned cargo tools (`cargo install --locked <tool> --version <version>`), print
    /// how to install gitleaks and lefthook, and check their versions against the pins.
    Setup {
        /// Also install gitleaks and lefthook where the version check fails, then check again.
        ///
        /// A binary already at its pin is left alone. On macOS and Linux the release asset is
        /// moved into `CARGO_HOME/bin` only if its sha256 matches the pin; on Windows winget runs.
        /// A failed install fails setup with a fix line. Versions are checked either way.
        #[arg(long)]
        binaries: bool,
    },
    /// Time the build loop against `budgets.toml`: the Rust half, the web half, or both (the
    /// default).
    Buildtime {
        /// Which half to time.
        #[arg(default_value = "all")]
        half: buildtime::Half,
        /// Timed runs per probe, after one warm-up run.
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
        runs: u32,
        /// Print one JSON object on stdout instead of the table.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Commands::Check { quick } => check::run(quick),
        Commands::Setup { binaries } => setup::run(binaries),
        Commands::Buildtime { half, runs, json } => buildtime::run(half, runs, json),
    }
}
