//! `cargo xtask buildtime [rust|web|all] [--runs N] [--json]`: times the edit loop of the app and
//! of the protocol crate (the Rust half), and of the frontend in `ui/app` (the web half,
//! `web.rs`), against the rows of `budgets.toml`. `all` runs the Rust half, then the web half.
//!
//! A probe writes the next value of its crate's `BUILDTIME_MARKER`, runs `cargo <sub> -v` as a
//! warm-up that is not counted, then times `cargo <sub>` runs. The warm-up's rustc lines are the
//! authority on whether the crate was rebuilt and whether it was built with incremental. The file
//! is put back after every probe, and at the start of the next run if this one was interrupted
//! (`probe.rs`), but never over an edit made during the run. The web half's edit of
//! `ui/app/src/App.tsx` is guarded by its script, under the rules written out in
//! `ui/app/scripts/guard.mjs`. One run at a time holds the checkout's lock (`lock.rs`),
//! through both halves. Only `clock::timed` reads the clock, and only
//! `process.rs` starts processes; the web half's Node script does its own timing.

pub(crate) mod lock;
mod marker;
mod probe;
mod profile;
mod report;
mod verdict;
mod web;

#[cfg(test)]
mod tests;

use crate::budgets;
use crate::check;
use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::workspace::{self, Member};
use clap::ValueEnum;
use profile::{Incremental, Sources};
use report::WebJson;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use verdict::{Failure, Outcome, ProbeResult, ProbeStatus, passes};
use web::WebRun;

/// The fix for a probe whose warm-up shows its crate builds without incremental in CI. Cargo turns
/// incremental off whenever CI is set, unless this is set.
const INCREMENTAL_FIX: &str =
    "set CARGO_INCREMENTAL=1 for the run (cargo turns incremental off whenever CI is set)";

/// The fix for a web probe over its budget that fails the run.
const WEB_OVER_FIX: &str = "find what slowed the loop (the tool's own output shows where the time goes); a limit in budgets.toml changes only with a reviewed measurement";

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

/// Reads `budgets.toml`. `Err` holds one line per problem; each is printed before it returns.
fn load_budgets(root: &Path, diag: &mut Out) -> Result<Vec<budgets::Budget>, Vec<String>> {
    budgets::load(root).map_err(|violations| {
        violations
            .iter()
            .map(|violation| {
                diag.violation(violation);
                one_line(&format!(
                    "{}:{}: {}",
                    violation.path, violation.line, violation.message
                ))
            })
            .collect()
    })
}

/// Runs the Rust half, under the checkout's lock that the caller holds. Puts back any file an
/// interrupted run left, then reads the budgets and the expected incremental setting, then times
/// the probes. Prints the rustc and profile lines when not `json`. `Err` holds one line per reason
/// the run cannot start; the reason is printed before it returns.
fn measure_rust(
    root: &Path,
    runs: u32,
    json: bool,
    out: &mut Out,
    diag: &mut Out,
) -> Result<RustRun, Vec<String>> {
    let leftovers = match probe::restore_leftovers(root) {
        Ok(lines) => lines,
        Err(failure) => return Err(stop(diag, &failure.message, &failure.fix)),
    };
    for line in &leftovers {
        diag.note(line);
    }
    let budgets = load_budgets(root, diag)?;
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
    let members = match workspace::load(root) {
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
            Planned::Skip(probe) => {
                rust_result(&probe, Outcome::Skipped("crate not present".into()))
            }
            Planned::Run { probe, dir } if kept.contains(&dir) => {
                rust_result(&probe, Outcome::Skipped(KEPT_COPY.into()))
            }
            Planned::Run { probe, dir } => {
                let outcome = probe::run_probe(root, &dir, &probe, &budgets, runs, ci, &mut runner);
                if matches!(outcome, Outcome::Failed(_)) && probe::holds_kept_copy(root, &dir) {
                    kept.push(dir);
                }
                rust_result(&probe, outcome)
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

/// A Rust probe's result: it measures seconds, and a failure or a gated overrun fails the run.
fn rust_result(probe: &Probe, outcome: Outcome) -> ProbeResult {
    ProbeResult {
        name: probe.name,
        unit: budgets::Unit::S,
        reported_only: false,
        outcome,
    }
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

/// Runs `cargo xtask buildtime`. Takes the checkout's lock for the whole run, then runs the Rust
/// half, the web half, or the Rust half then the web half. Returns the exit code.
pub(crate) fn run(half: Half, runs: u32, json: bool) -> ExitCode {
    let root = workspace::workspace_root();
    let mut out: Out = Printer::new(Box::new(io::stdout()), false);
    let sink: Box<dyn Write> = if json {
        Box::new(io::stderr())
    } else {
        Box::new(io::stdout())
    };
    let mut diag: Out = Printer::new(sink, false);
    let stopped = |out: &mut Out, errors: &[String]| {
        if json {
            out.note(report::error_json(errors));
        }
        ExitCode::FAILURE
    };

    let _lock = match lock::acquire(&root) {
        Ok(file) => file,
        Err(failure) => {
            let errors = stop(&mut diag, &failure.message, &failure.fix);
            return stopped(&mut out, &errors);
        }
    };
    let rust = if half == Half::Web {
        None
    } else {
        match measure_rust(&root, runs, json, &mut out, &mut diag) {
            Ok(rust) => Some(rust),
            Err(errors) => return stopped(&mut out, &errors),
        }
    };
    if let Some(rust) = &rust {
        if !json {
            for line in report::table_lines(&rust.results) {
                out.note(line);
            }
            for line in report::warning_lines(&rust.results) {
                out.note(line);
            }
        }
        report::print_failures(&rust.results, &mut diag);
        report_over_budget(&rust.results, rust.ci, OVER_FIX, &mut diag);
    }
    let web = (half != Half::Rust).then(|| {
        let ci = check::is_ci(env::var("CI").ok().as_deref());
        WebJson::of(&measure_web(&root, runs, ci, json, &mut out, &mut diag))
    });
    let rust_passed = rust
        .as_ref()
        .is_none_or(|rust| passes(&rust.results, rust.ci));
    let passed = rust_passed && web.as_ref().is_none_or(WebJson::passed);
    if json {
        match (&rust, &web) {
            (Some(rust), web) => out.note(report::json_object(rust, passed, web.as_ref())),
            (None, Some(web)) => out.note(report::web_json(web)),
            (None, None) => {}
        }
    }
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Runs the web half: reads the budgets, runs the script, then prints the node line, the table and
/// the reported-only line when not `json`, and the failures and over-budget probes to `diag`.
fn measure_web(
    root: &Path,
    runs: u32,
    ci: bool,
    json: bool,
    out: &mut Out,
    diag: &mut Out,
) -> Result<WebRun, Vec<String>> {
    let budgets = load_budgets(root, diag)?;
    let run = web::measure(root, runs, ci, &budgets, diag)?;
    if !json {
        out.note(format!("web: node {}", run.node));
        for line in report::table_lines(&run.results) {
            out.note(line);
        }
        if let Some(line) = web::reported_line(web::WEB_PROBES) {
            out.note(line);
        }
    }
    report::print_failures(&run.results, diag);
    for failure in &run.failures {
        diag.failure(&failure.message, &failure.fix);
    }
    report_over_budget(&run.results, ci, WEB_OVER_FIX, diag);
    Ok(run)
}

/// Names the over-budget probes. Those that fail the run get a failure with `fix`. Those only
/// reported get a note: "CI reports only" in CI, and "reported only" locally, for an alert gate or
/// a reported-only probe.
fn report_over_budget(results: &[ProbeResult], ci: bool, fix: &str, diag: &mut Out) {
    let over: Vec<(&str, bool)> = results
        .iter()
        .filter(|result| result.status() == ProbeStatus::Over)
        .map(|result| (result.name, result.fails_run(ci)))
        .collect();
    let failing: Vec<&str> = over
        .iter()
        .filter(|(_, fails)| *fails)
        .map(|(name, _)| *name)
        .collect();
    let reported: Vec<&str> = over
        .iter()
        .filter(|(_, fails)| !*fails)
        .map(|(name, _)| *name)
        .collect();
    if !reported.is_empty() {
        let names = reported.join(", ");
        if ci {
            diag.note(format!("over budget, CI reports only: {names}"));
        } else {
            diag.note(format!("over budget, reported only: {names}"));
        }
    }
    if !failing.is_empty() {
        diag.failure(&format!("over budget: {}", failing.join(", ")), fix);
    }
}
