//! `cargo xtask buildtime [rust|web|all] [--runs N] [--json]`: times the edit loop of the app and of
//! the protocol crate against the rows of `budgets.toml`. The web half is not built until R4.
//!
//! A probe writes the next value of its crate's `BUILDTIME_MARKER`, runs `cargo <sub> -v` as a
//! warm-up that is not counted, then times `cargo <sub>` runs. The warm-up's rustc lines are the
//! authority on whether the crate was rebuilt and whether it was built with incremental. The file
//! is put back after every probe, and at the start of the next run if this one was interrupted
//! (`probe.rs`), but never over an edit made during the run. One run at a time holds the checkout's
//! lock (`lock.rs`). Only `clock::timed` reads the clock, and only `process.rs` starts processes.

mod lock;
mod marker;
mod probe;
mod profile;
mod report;
#[cfg(test)]
mod scratch;
mod verdict;

#[cfg(test)]
mod tests;

use crate::budgets;
use crate::check;
use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace::{self, Member};
use clap::ValueEnum;
use profile::{Incremental, Sources};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use verdict::{Failure, Outcome, ProbeResult, ProbeStatus, passes};

/// The web half's message, until R4 builds it.
const WEB_MESSAGE: &str = "not implemented until R4";

/// The fix for a probe whose warm-up shows its crate builds without incremental in CI. Cargo turns incremental off whenever CI is set, unless this is set.
const INCREMENTAL_FIX: &str =
    "set CARGO_INCREMENTAL=1 for the run (cargo turns incremental off whenever CI is set)";

/// The fix for a probe over its budget that fails the run.
const OVER_FIX: &str = "find what slowed the loop (`cargo build --timings` shows where the time goes); a limit in budgets.toml changes only with a reviewed measurement";

/// Which half of the build loop to time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Half {
    Rust,
    Web,
    All,
}

/// One timed edit loop: `cargo <subcommand>` in `package`, against the budget row `budget_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Probe {
    pub name: &'static str,
    pub package: &'static str,
    pub budget_id: &'static str,
    pub subcommand: &'static str,
}

/// The four probes, in the order they run and print.
const PROBES: &[Probe] = &[
    Probe {
        name: "check_desktop",
        package: "clauseo-desktop",
        budget_id: "cargo_check_app_edit",
        subcommand: "check",
    },
    Probe {
        name: "build_desktop",
        package: "clauseo-desktop",
        budget_id: "cargo_build_app_edit",
        subcommand: "build",
    },
    Probe {
        name: "check_protocol",
        package: "clauseo-protocol",
        budget_id: "cargo_check_protocol_edit",
        subcommand: "check",
    },
    Probe {
        name: "build_protocol",
        package: "clauseo-protocol",
        budget_id: "cargo_build_protocol_edit",
        subcommand: "build",
    },
];

/// What a probe does on this machine. `Run` carries the member's directory, where the marker file
/// is. Cargo itself runs at the repo root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Planned {
    Run { probe: Probe, dir: String },
    Skip(Probe),
}

/// Each probe, run when its package is a workspace member and skipped otherwise.
pub(crate) fn plan(probes: &[Probe], members: &[Member]) -> Vec<Planned> {
    probes
        .iter()
        .map(
            |probe| match members.iter().find(|member| member.name == probe.package) {
                Some(member) => Planned::Run {
                    probe: *probe,
                    dir: member.dir.clone(),
                },
                None => Planned::Skip(*probe),
            },
        )
        .collect()
}

/// The Rust half, once it has been measured.
pub(crate) struct RustRun {
    rustc: String,
    incremental: Incremental,
    ci: bool,
    runs: u32,
    results: Vec<ProbeResult>,
}

/// Prints a failure that stops the run, and returns its line for the JSON errors.
fn stop(diag: &mut Out, message: &str, fix: &str) -> Vec<String> {
    diag.failure(message, fix);
    vec![one_line(message)]
}

/// `text` on one line: every run of whitespace, newlines included, becomes one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Runs the Rust half. Takes the checkout's lock, puts back any file an interrupted run left, then
/// reads the budgets and the expected incremental setting, then times the probes, holding the lock
/// to the end. Prints the rustc and profile lines when not `json`. `Err` holds one line per reason
/// the run cannot start; the reason is printed before it returns.
fn measure_rust(
    root: &Path,
    runs: u32,
    json: bool,
    out: &mut Out,
    diag: &mut Out,
) -> Result<RustRun, Vec<String>> {
    let _lock = match lock::acquire(&root.join(lock::LOCK_PATH)) {
        Ok(file) => file,
        Err(failure) => return Err(stop(diag, &failure.message, &failure.fix)),
    };
    let leftovers = match probe::restore_leftovers(root) {
        Ok(lines) => lines,
        Err(failure) => return Err(stop(diag, &failure.message, &failure.fix)),
    };
    for line in &leftovers {
        diag.note(line);
    }
    let budgets = match budgets::load(root) {
        Ok(budgets) => budgets,
        Err(violations) => {
            let mut errors = Vec::new();
            for violation in &violations {
                diag.violation(violation);
                errors.push(one_line(&format!(
                    "{}:{}: {}",
                    violation.path, violation.line, violation.message
                )));
            }
            return Err(errors);
        }
    };
    let rustc = match rustc_version(root) {
        Ok(version) => version,
        Err(failure) => return Err(stop(diag, &failure.message, &failure.fix)),
    };
    let manifest = match fs::read_to_string(root.join("Cargo.toml")) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(stop(
                diag,
                &format!("cannot read Cargo.toml: {error}"),
                "restore Cargo.toml at the repo root",
            ));
        }
    };
    let cargo_incremental = env::var("CARGO_INCREMENTAL").ok();
    let build_incremental = env::var("CARGO_BUILD_INCREMENTAL").ok();
    let profile_dev_incremental = env::var("CARGO_PROFILE_DEV_INCREMENTAL").ok();
    let config_toml = fs::read_to_string(root.join(".cargo/config.toml")).ok();
    let incremental = profile::resolve(&Sources {
        cargo_incremental: cargo_incremental.as_deref(),
        build_incremental: build_incremental.as_deref(),
        ci_present: env::var_os("CI").is_some(),
        profile_dev_incremental: profile_dev_incremental.as_deref(),
        config_toml: config_toml.as_deref(),
        manifest: Some(&manifest),
    });
    let ci = check::is_ci(env::var("CI").ok().as_deref());
    if !json {
        out.note(&rustc);
        out.note(profile_line(&incremental));
    }
    let members = match workspace::load_members(root) {
        Ok(members) => members,
        Err(error) => {
            return Err(stop(diag, &error, "run `cargo metadata` to see the error"));
        }
    };
    let mut runner = |cmd: &Cmd| process::capture(cmd, root, &mut |_: &[u8]| {});
    // Crates whose saved copy a failed probe kept. Later probes on them are skipped, so the run
    // reports the conflict once and never prints a second fix that could undo it.
    let mut kept: Vec<String> = Vec::new();
    let results = plan(PROBES, &members)
        .into_iter()
        .map(|planned| match planned {
            Planned::Skip(probe) => ProbeResult {
                name: probe.name,
                outcome: Outcome::Skipped("crate not present"),
            },
            Planned::Run { probe, dir } if kept.contains(&dir) => ProbeResult {
                name: probe.name,
                outcome: Outcome::Skipped(KEPT_COPY),
            },
            Planned::Run { probe, dir } => {
                let outcome = probe::run_probe(root, &dir, &probe, &budgets, runs, ci, &mut runner);
                if matches!(outcome, Outcome::Failed(_)) && probe::holds_kept_copy(root, &dir) {
                    kept.push(dir);
                }
                ProbeResult {
                    name: probe.name,
                    outcome,
                }
            }
        })
        .collect();
    Ok(RustRun {
        rustc,
        incremental,
        ci,
        runs,
        results,
    })
}

/// Why a probe is skipped after an earlier probe on its crate kept the saved copy.
const KEPT_COPY: &str = "an earlier probe on this crate kept its saved copy; resolve that first";

/// The profile line. The resolver reads only some of cargo's config files, so its answer is an
/// expectation; each probe's warm-up rustc line decides, and refuses a non-incremental build in CI.
fn profile_line(incremental: &Incremental) -> String {
    let state = if incremental.on { "on" } else { "off" };
    format!(
        "profile: dev, incremental expected {state} ({}); each probe's warm-up rustc line decides",
        incremental.source
    )
}

/// `rustc -V`, trimmed.
fn rustc_version(root: &Path) -> Result<String, Failure> {
    let cmd = Cmd::new(Program::Named("rustc"), ["-V"]);
    match process::run_quiet(&cmd, root) {
        Ok(quiet) if quiet.success => Ok(quiet.stdout.trim().to_owned()),
        Ok(quiet) => Err(Failure::new(
            format!("`rustc -V` failed: {}", quiet.stderr.trim()),
            "install Rust with rustup, then run `rustup show`",
        )),
        Err(error) => Err(Failure::new(
            format!("could not run `rustc -V`: {error}"),
            "install Rust with rustup",
        )),
    }
}

/// A printer for lines that may be redirected: stdout for the table, stderr for JSON-mode errors.
type Out = Printer<Box<dyn Write>>;

/// Runs `cargo xtask buildtime`. Returns the exit code.
pub(crate) fn run(half: Half, runs: u32, json: bool) -> ExitCode {
    let root = workspace::workspace_root();
    let mut out: Out = Printer::new(Box::new(io::stdout()), false);
    let sink: Box<dyn Write> = if json {
        Box::new(io::stderr())
    } else {
        Box::new(io::stdout())
    };
    let mut diag: Out = Printer::new(sink, false);

    if half == Half::Web {
        if json {
            out.note(report::web_json());
        } else {
            out.note(format!("web: {WEB_MESSAGE}"));
        }
        return ExitCode::SUCCESS;
    }
    let rust = match measure_rust(&root, runs, json, &mut out, &mut diag) {
        Ok(rust) => rust,
        Err(errors) => {
            if json {
                out.note(report::error_json(&errors));
            }
            return ExitCode::FAILURE;
        }
    };
    let passed = passes(&rust.results, rust.ci);
    if json {
        out.note(report::json_object(&rust, passed, half == Half::All));
    } else {
        for line in report::table_lines(&rust.results) {
            out.note(line);
        }
        for line in report::warning_lines(&rust.results) {
            out.note(line);
        }
        if half == Half::All {
            out.note(format!("web: {WEB_MESSAGE}"));
        }
    }
    report::print_failures(&rust.results, &mut diag);
    report_over_budget(&rust.results, rust.ci, &mut diag);
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Names the over-budget probes. Those that fail the run get a failure with its fix. Those only
/// reported get a note: "CI reports only" in CI, and "reported only" locally, for an alert gate.
fn report_over_budget(results: &[ProbeResult], ci: bool, diag: &mut Out) {
    let (failing, reported): (Vec<&ProbeResult>, Vec<&ProbeResult>) = results
        .iter()
        .filter(|result| result.outcome.status() == ProbeStatus::Over)
        .partition(|result| result.outcome.fails_run(ci));
    if !reported.is_empty() {
        let names = names(&reported);
        if ci {
            diag.note(format!("over budget, CI reports only: {names}"));
        } else {
            diag.note(format!("over budget, reported only: {names}"));
        }
    }
    if !failing.is_empty() {
        diag.failure(&format!("over budget: {}", names(&failing)), OVER_FIX);
    }
}

/// The probe names, comma-separated.
fn names(results: &[&ProbeResult]) -> String {
    results
        .iter()
        .map(|result| result.name)
        .collect::<Vec<_>>()
        .join(", ")
}
