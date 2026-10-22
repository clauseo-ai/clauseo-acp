//! Workspace automation: `cargo xtask check [--quick]`,
//! `cargo xtask setup [--binaries] [--only <tool>]... | --list`,
//! `cargo xtask buildtime [rust|web|all] [--runs N] [--json]` and `cargo xtask mutants`.
//!
//! For people and agents:
//! - Add an external step: one `ToolStep` in `steps/plan.rs`, or `steps/policy.rs` for a
//!   repo-policy tool. Its `parse` turns the output into findings.
//! - Add a static check: a new file in `src/checks/` with a `StaticCheck`, plus one line in
//!   `checks::ALL`. Its `run` returns an `Inspection` that counts what it inspected (zero fails),
//!   and its planted cases go in one `checks::planted::cases!` list.
//! - Pin a tool: one `Tool` in `setup::TOOLS` for a cargo crate, else one `Binary` in
//!   `setup::BINARIES`.
//! - `setup` prints a `Binary`'s install command; `setup --binaries` also runs it. `setup --only
//!   <tool>`, repeatable, sets up only the named `Tool`s and `Binary`s. `setup --list` prints
//!   every pin in the tab-separated format the `setup.rs` module doc writes out.
//! - Budgets live in `budgets.toml` at the repo root; `budgets.rs` reads them for the check and for
//!   `buildtime`.
//! - Failures print as `path:line: what`, then `    fix: ...`. A failure with no location prints
//!   the tool's output once, then a `fix:` line.
//! - Live tool output appears only when stdout is a terminal. Otherwise a passing tool prints its
//!   header and our findings only.
//! - Only `process.rs` starts processes and only `output.rs` prints. Keep it that way. Every
//!   command gets a time limit in `Cmd::new`; `process.rs` says how its tree is stopped. The one
//!   clock read (`clock.rs`) and the one `Command::spawn` (`spawn` in `process.rs`) carry
//!   `#[expect(clippy::disallowed_methods, reason = "...")]`.
//! - Two subcommands are hidden from `--help`: `process-leader`, the leader `process.rs` starts
//!   on Unix, and `process-test`, which `xtask/tests/process.rs` runs.
//! - `buildtime web` runs `node ui/app/scripts/buildtime.mjs`, which times the
//!   `ui/app/package.json` scripts with Node's own clock and prints JSON (the contract is in
//!   `buildtime/web.rs`); xtask computes the statistics and judges them against budgets.
//! - `mutants` runs cargo-mutants on the diff against origin/main and reads its `outcomes.json`
//!   (`mutants/outcomes.rs`); exclusions live in `.cargo/mutants.toml`, each with its reason.

mod budgets;
mod buildtime;
mod changes;
mod check;
mod checks;
mod clock;
mod fix;
mod mutants;
mod output;
mod parse;
mod process;
#[cfg(test)]
mod scratch;
mod setup;
mod steps;
mod violation;
mod workspace;
mod write;

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
    /// how to install gitleaks and lefthook, and check their versions against the pins. `--only`
    /// narrows all of this to the named tools; `--list` does none of it and prints the pins.
    Setup {
        /// Also install the selected binaries (gitleaks and lefthook unless `--only` narrows them)
        /// where the version check fails, then check again.
        ///
        /// A binary already at its pin is left alone. On macOS and Linux the release asset is
        /// moved into `CARGO_HOME/bin` only if its sha256 matches the pin; on Windows winget runs.
        /// A failed install fails setup with a fix line. Versions are checked either way.
        #[arg(long)]
        binaries: bool,
        /// Set up only this pinned tool, named as `setup` prints it (`cargo-nextest`,
        /// `gitleaks`, ...). Repeat it to name more. An unknown name fails before any install.
        #[arg(long, value_name = "TOOL")]
        only: Vec<String>,
        /// Print every pin and install and check nothing, one line each, fields separated by tabs.
        ///
        /// `tool<TAB><name><TAB><version>` for each pinned cargo tool, then
        /// `binary<TAB><name><TAB><version><TAB><os><TAB><arch><TAB><sha256>` for each pinned
        /// release asset of gitleaks and lefthook (`os` and `arch` as in Rust's
        /// `std::env::consts`, `sha256` in lowercase hex), including each binary's `windows`
        /// `x86_64` asset, which `--binaries` does not download (it runs winget there).
        #[arg(long, conflicts_with_all = ["binaries", "only"])]
        list: bool,
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
    /// Mutation-test the lines changed since `git merge-base HEAD origin/main` with cargo-mutants
    /// (settings in `.cargo/mutants.toml`). Each missed mutant fails as `path:line: what` and
    /// `fix:`. A run is capped at 20 minutes.
    Mutants,
    /// The leader `process.rs` starts on Unix: stops the process groups it is told about when
    /// its stdin closes.
    #[cfg(unix)]
    #[command(hide = true)]
    ProcessLeader,
    /// Runs `sh -c <script>` through the process door, for `xtask/tests/process.rs`.
    #[command(hide = true)]
    ProcessTest {
        output: process::TestOutput,
        script: String,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Commands::Check { quick } => {
            let root = workspace::workspace_root();
            let ctx = workspace::Context {
                members: workspace::load(&root),
                root: root.clone(),
                load_resolved_graph: true,
            };
            let ci = check::is_ci(std::env::var("CI").ok().as_deref());
            let mut exec = |cmd: &process::Cmd, on_output: &mut dyn FnMut(&[u8])| {
                process::capture(cmd, &root, on_output)
            };
            check::run(&ctx, quick, ci, &mut exec, &mut process::run_quiet)
        }
        Commands::Setup { list: true, .. } => setup::list(&mut output::Printer::stdout()),
        Commands::Setup { binaries, only, .. } => setup::run(
            &workspace::workspace_root(),
            setup::Selection::of(&only),
            binaries,
            &mut process::run,
        ),
        Commands::Buildtime { half, runs, json } => {
            buildtime::run(&workspace::workspace_root(), half, runs, json)
        }
        Commands::Mutants => mutants::run(&workspace::workspace_root()),
        #[cfg(unix)]
        Commands::ProcessLeader => process::lead(),
        Commands::ProcessTest { output, script } => process::test_mode(output, &script),
    }
}
