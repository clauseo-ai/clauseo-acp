//! `cargo xtask buildtime [rust|web|all] [--runs N] [--json]`: times the edit loop of the app and of
//! the protocol crate against the rows of `budgets.toml`. The web half is not built until R4.
//!
//! A probe writes the next value of its crate's `BUILDTIME_MARKER`, runs cargo, and times it. The
//! first run is a warm-up and is not counted. The file is restored after every probe, even when
//! cargo fails. Only `clock::timed` reads the clock, and only `process.rs` starts processes.

mod marker;
mod profile;
mod report;

#[cfg(test)]
mod tests;

use crate::budgets::{self, Budget, Unit};
use crate::check;
use crate::clock::timed;
use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace::{self, Member};
use clap::ValueEnum;
use marker::{Marker, bump_marker};
use profile::Incremental;
use serde::Serialize;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// The web half's message, until R4 builds it.
const WEB_MESSAGE: &str = "not implemented until R4";

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

/// What a probe does on this machine: run in the member's directory, or be skipped.
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

/// The median, minimum and maximum of the timed runs, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stats {
    pub median: f64,
    pub min: f64,
    pub max: f64,
}

/// The stats of `samples`: the middle value, or the mean of the middle two for an even count.
/// `None` when there are no samples.
pub(crate) fn stats(samples: &[f64]) -> Option<Stats> {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let min = *sorted.first()?;
    let max = *sorted.last()?;
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[middle - 1], sorted[middle])
    } else {
        sorted[middle]
    };
    Some(Stats { median, min, max })
}

/// Why a probe failed: what happened, the fix line, and cargo's output when cargo ran and failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub message: String,
    pub fix: String,
    pub output: Option<String>,
}

impl Failure {
    fn new(message: impl Into<String>, fix: impl Into<String>) -> Failure {
        Failure {
            message: message.into(),
            fix: fix.into(),
            output: None,
        }
    }
}

/// The status of a probe, as the table and the JSON print it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProbeStatus {
    Skipped,
    Ok,
    Over,
    Failed,
}

impl ProbeStatus {
    fn word(self) -> &'static str {
        match self {
            ProbeStatus::Skipped => "skipped",
            ProbeStatus::Ok => "ok",
            ProbeStatus::Over => "over",
            ProbeStatus::Failed => "failed",
        }
    }
}

/// What one probe came to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    Skipped(&'static str),
    Failed(Failure),
    Measured {
        stats: Stats,
        budget_id: &'static str,
        budget_s: f64,
    },
}

impl Outcome {
    /// `ok` when the median is within the budget, `over` when it is not.
    pub(crate) fn status(&self) -> ProbeStatus {
        match self {
            Outcome::Skipped(_) => ProbeStatus::Skipped,
            Outcome::Failed(_) => ProbeStatus::Failed,
            Outcome::Measured {
                stats, budget_s, ..
            } => {
                if stats.median <= *budget_s {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Over
                }
            }
        }
    }
}

/// One probe's name and outcome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeResult {
    pub name: &'static str,
    pub outcome: Outcome,
}

/// The limit of the budget row `id`, in seconds. A missing row, or a unit other than `s` or `ms`,
/// fails the probe.
pub(crate) fn budget_seconds(budgets: &[Budget], id: &str) -> Result<f64, Failure> {
    let Some(budget) = budgets.iter().find(|budget| budget.id == id) else {
        return Err(Failure::new(
            format!("no budget row {id} in budgets.toml"),
            format!("add a [[budget]] row with id = \"{id}\" to budgets.toml"),
        ));
    };
    match budget.unit {
        Unit::S => Ok(budget.limit),
        Unit::Ms => Ok(budget.limit / 1000.0),
        other => Err(Failure::new(
            format!(
                "budget {id} is in {}, and buildtime compares s or ms",
                other.label()
            ),
            format!("set unit = \"s\" or \"ms\" on budget {id}"),
        )),
    }
}

/// Runs the cargo command of `probe` against the marker in `dir`, and returns its outcome.
fn run_probe(root: &Path, dir: &str, probe: &Probe, budgets: &[Budget], runs: u32) -> Outcome {
    let budget_s = match budget_seconds(budgets, probe.budget_id) {
        Ok(seconds) => seconds,
        Err(failure) => return Outcome::Failed(failure),
    };
    let Some(marker) = marker::find(root, dir) else {
        let lib = marker::in_crate(dir, "src/lib.rs");
        return Outcome::Failed(Failure::new(
            format!("{lib}:1: no pub const BUILDTIME_MARKER: u64 in src/lib.rs or src/main.rs"),
            "add pub const BUILDTIME_MARKER: u64 = 0; to the crate root",
        ));
    };
    match measure(root, &marker, probe.subcommand, runs) {
        Err(failure) => Outcome::Failed(failure),
        Ok(samples) => match stats(&samples) {
            Some(stats) => Outcome::Measured {
                stats,
                budget_id: probe.budget_id,
                budget_s,
            },
            None => Outcome::Failed(Failure::new(
                format!("no timed runs for {}", probe.name),
                "run `cargo xtask buildtime` with --runs 1 or more",
            )),
        },
    }
}

/// Runs the warm-up and the timed runs of `cargo <subcommand>`, then restores the marker file.
/// Each run writes the next value, so cargo rebuilds the crate each time.
fn measure(root: &Path, marker: &Marker, subcommand: &str, runs: u32) -> Result<Vec<f64>, Failure> {
    let path = root.join(&marker.path);
    let cmd = Cmd::new(Program::Cargo, [subcommand]);
    let mut samples = Vec::new();
    let edited = edit_and_run(&path, marker, &cmd, root, runs, &mut samples);
    let restored = std::fs::write(&path, &marker.text);
    match (edited, restored) {
        (Ok(()), Ok(())) => Ok(samples),
        (Ok(()), Err(error)) => Err(Failure::new(
            format!("could not restore {}: {error}", marker.path),
            format!(
                "restore {} from git: git checkout -- {}",
                marker.path, marker.path
            ),
        )),
        (Err(failure), Ok(())) => Err(failure),
        (Err(mut failure), Err(error)) => {
            failure.message = format!(
                "{}; and could not restore {}: {error}",
                failure.message, marker.path
            );
            Err(failure)
        }
    }
}

/// The loop of `measure`: one warm-up run, then `runs` timed runs, each with a new marker value.
fn edit_and_run(
    path: &Path,
    marker: &Marker,
    cmd: &Cmd,
    root: &Path,
    runs: u32,
    samples: &mut Vec<f64>,
) -> Result<(), Failure> {
    let mut current = marker.text.clone();
    for run in 0..=runs {
        current = bump_marker(&current).ok_or_else(|| {
            Failure::new(
                format!("{} lost its marker line", marker.path),
                "restore the BUILDTIME_MARKER line by hand",
            )
        })?;
        std::fs::write(path, &current).map_err(|error| {
            Failure::new(
                format!("could not write {}: {error}", marker.path),
                "make sure the file is writable",
            )
        })?;
        let (captured, secs) = timed(|| process::capture(cmd, root, &mut |_: &[u8]| {}));
        let captured = captured.map_err(|error| {
            Failure::new(
                format!("could not run `{cmd}`: {error}"),
                format!("run `{cmd}` to see the error"),
            )
        })?;
        if !captured.success {
            return Err(Failure {
                message: format!("{cmd} failed"),
                fix: format!("run `{cmd}` to see the error"),
                output: Some(captured.output),
            });
        }
        if run > 0 {
            samples.push(secs);
        }
    }
    Ok(())
}

/// Whether a run passes. Any failed probe fails it. An over-budget probe fails it locally, and
/// passes it in CI, where the table is reported and not enforced.
pub(crate) fn passes(statuses: &[ProbeStatus], ci: bool) -> bool {
    if statuses.contains(&ProbeStatus::Failed) {
        return false;
    }
    ci || !statuses.contains(&ProbeStatus::Over)
}

/// The Rust half, once it has been measured.
struct RustRun {
    rustc: String,
    incremental: Incremental,
    ci: bool,
    results: Vec<ProbeResult>,
}

/// Runs the Rust half. Prints the rustc and profile lines when not `json`. Returns `None` after
/// printing the reason when the run cannot start.
fn measure_rust(
    root: &Path,
    runs: u32,
    json: bool,
    out: &mut Out,
    diag: &mut Out,
) -> Option<RustRun> {
    let budgets = match budgets::load(root) {
        Ok(budgets) => budgets,
        Err(violations) => {
            for violation in &violations {
                diag.violation(violation);
            }
            return None;
        }
    };
    let rustc = match rustc_version(root) {
        Ok(version) => version,
        Err(failure) => {
            diag.failure(&failure.message, &failure.fix);
            return None;
        }
    };
    let manifest = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(manifest) => manifest,
        Err(error) => {
            diag.failure(
                &format!("cannot read Cargo.toml: {error}"),
                "restore Cargo.toml at the repo root",
            );
            return None;
        }
    };
    let incremental = profile::resolve(
        std::env::var("CARGO_INCREMENTAL").ok().as_deref(),
        std::env::var("CARGO_PROFILE_DEV_INCREMENTAL")
            .ok()
            .as_deref(),
        &manifest,
    );
    let ci = check::is_ci(std::env::var("CI").ok().as_deref());
    if !json {
        out.note(&rustc);
        let state = if incremental.on { "on" } else { "off" };
        out.note(format!(
            "profile: dev, incremental {state} ({})",
            incremental.source
        ));
    }
    if ci && !incremental.on {
        diag.failure(
            &format!(
                "refusing to measure: CI is set and incremental is off ({})",
                incremental.source
            ),
            "unset CARGO_INCREMENTAL (or set it to 1) and keep incremental = true in [profile.dev]",
        );
        return None;
    }
    let members = match workspace::load_members(root) {
        Ok(members) => members,
        Err(error) => {
            diag.failure(&error, "run `cargo metadata` to see the error");
            return None;
        }
    };
    let results = plan(PROBES, &members)
        .into_iter()
        .map(|planned| match planned {
            Planned::Skip(probe) => ProbeResult {
                name: probe.name,
                outcome: Outcome::Skipped("crate not present"),
            },
            Planned::Run { probe, dir } => ProbeResult {
                name: probe.name,
                outcome: run_probe(root, &dir, &probe, &budgets, runs),
            },
        })
        .collect();
    Some(RustRun {
        rustc,
        incremental,
        ci,
        results,
    })
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
    let Some(rust) = measure_rust(&root, runs, json, &mut out, &mut diag) else {
        return ExitCode::FAILURE;
    };
    if json {
        out.note(report::json_object(&rust, half == Half::All));
    } else {
        for line in report::table_lines(&rust.results) {
            out.note(line);
        }
        if half == Half::All {
            out.note(format!("web: {WEB_MESSAGE}"));
        }
    }
    report::print_failures(&rust.results, &mut diag);

    let statuses: Vec<ProbeStatus> = rust.results.iter().map(|r| r.outcome.status()).collect();
    let over: Vec<&str> = rust
        .results
        .iter()
        .filter(|r| r.outcome.status() == ProbeStatus::Over)
        .map(|r| r.name)
        .collect();
    if !over.is_empty() {
        if rust.ci {
            diag.note("over budget: CI only reports it");
        } else {
            diag.failure(
                &format!("over budget: {}", over.join(", ")),
                "find what slowed the loop (`cargo build --timings` shows where the time goes); a limit in budgets.toml changes only with a reviewed measurement",
            );
        }
    }
    if passes(&statuses, rust.ci) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
