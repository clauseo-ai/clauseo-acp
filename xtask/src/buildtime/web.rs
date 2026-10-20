//! The web half: `node ui/app/scripts/buildtime.mjs --runs N` measures the frontend's edit loop
//! and reports it as JSON; this module reads it, computes the statistics with `verdict::stats`,
//! compares them with the rows of `budgets.toml`, and shows the result.
//!
//! The script's output contract, kept here only (the script points at it):
//! - stdout: the last non-empty line is one JSON object. Lines before it are ignored, so a stray
//!   line from a library cannot break the run.
//!   `{"node": "v24.x", "runs": N, "probes": [...], "failures": [...]}`. Each probe is
//!   `{"probe": name}` plus exactly one of `"samples_ms": [numbers]` (timed runs, warm-up left
//!   out), `"bytes": number` (with `"files"`), `"skipped": "why"` or
//!   `"failed": {"message", "command", "output"}`, where `command` reproduces the failure.
//!   `failures` is optional: `[{"message", "fix"}]` for what is no probe's (a conflict over
//!   `ui/app/src/App.tsx`, a restore that failed); each fails the run. With `probes` empty, the
//!   script stopped before it measured (a conflict left by an interrupted run, or a Node older
//!   than `engines.node` in `ui/package.json` allows).
//! - exit status: 0, or 1 when it reports `failures`. A crash exits 1 with no JSON line.
//! - stderr: notes for people, printed as they are.
//!
//! The script times `tsc --noEmit`, `oxlint --type-aware` and `vite build` (one warm-up, then N
//! runs each), sizes the initial JS of that build, starts the Vite dev server on a free port
//! (`dev_start`), and times N edits of `src/App.tsx` until headless Chrome shows them (`hmr`). It
//! stops only the processes it started, each with its process group (`buildtime-proc.mjs`).
//!
//! The `hmr` probe edits App.tsx under the scripts' guard, `ui/app/scripts/guard.mjs`, where its
//! rules (the copy, the restore, the leftover rule) are written out. `BACKUP_DIR` in
//! `ui/app/scripts/buildtime-lib.mjs` puts the copy under `target/xtask/buildtime/`, and names the
//! leftover edit that `probe::restore_leftovers` leaves to the script. The lock (`lock.rs`) covers
//! the copy when xtask runs the script.
//!
//! Gating: `tsc`, `oxlint_type_aware` and `vite_build` follow their row's gate like the Rust
//! probes, and fail the run when they fail (the tool exits non-zero). `initial_js`, `dev_start`
//! and `hmr` are reported only: over budget or failed, they show in the table, a failure prints
//! its lines, and the run still passes. A skipped probe (no Chrome) never fails the run.

#[cfg(test)]
mod tests;

use super::verdict::{Failure, Outcome, ProbeResult, budget_limit, passes};
use super::{Out, one_line, stop};
use crate::budgets::{Budget, Unit};
use crate::process::{self, Cmd, Program};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// How long the script may run: it builds the app and starts the dev server once per run plus a
/// warm-up, each a cold-ish build on a slow machine, so this is xtask's longest limit with
/// `cargo install`'s.
const SCRIPT_TIMEOUT: Duration = Duration::from_hours(1);

/// The script, from the repo root.
pub(crate) const SCRIPT: &str = "ui/app/scripts/buildtime.mjs";

/// Whether a probe can fail the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gating {
    /// Its budget row's gate decides, as for the Rust probes, and a failure fails the run.
    Row,
    /// Reported, never fails the run: not over budget, and not when it fails.
    ReportedOnly,
}

/// One web probe: the script's name for it, its budget row, its unit and its gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WebProbe {
    pub name: &'static str,
    pub budget_id: Option<&'static str>,
    pub unit: Unit,
    pub gating: Gating,
}

/// The six probes, in the order the script runs them and the table prints them.
pub(crate) const WEB_PROBES: &[WebProbe] = &[
    WebProbe {
        name: "tsc",
        budget_id: Some("tsc_noemit"),
        unit: Unit::S,
        gating: Gating::Row,
    },
    WebProbe {
        name: "oxlint_type_aware",
        budget_id: Some("oxlint_type_aware"),
        unit: Unit::S,
        gating: Gating::Row,
    },
    WebProbe {
        name: "vite_build",
        budget_id: Some("vite_build_warm"),
        unit: Unit::S,
        gating: Gating::Row,
    },
    // Reported only, although its `budgets.toml` row says `gate = "pr"`: it stays reported only
    // until the app milestone's first transcript PR. Turning the gate on means changing this one
    // entry to `Gating::Row`; the reported-only line and the JSON follow from it.
    WebProbe {
        name: "initial_js",
        budget_id: Some("initial_js"),
        unit: Unit::Kb,
        gating: Gating::ReportedOnly,
    },
    WebProbe {
        name: "dev_start",
        budget_id: None,
        unit: Unit::Ms,
        gating: Gating::ReportedOnly,
    },
    WebProbe {
        name: "hmr",
        budget_id: Some("hot_reload_component_edit"),
        unit: Unit::Ms,
        gating: Gating::ReportedOnly,
    },
];

/// The line under the web table that names the probes of `probes` that never fail the run, or
/// `None` when every probe can.
pub(crate) fn reported_line(probes: &[WebProbe]) -> Option<String> {
    let names: Vec<&str> = probes
        .iter()
        .filter(|probe| probe.gating == Gating::ReportedOnly)
        .map(|probe| probe.name)
        .collect();
    (!names.is_empty()).then(|| format!("reported only, never fail the run: {}", names.join(", ")))
}

/// The object the script prints on stdout.
#[derive(Debug, Deserialize)]
pub(crate) struct ScriptReport {
    pub node: String,
    pub probes: Vec<ScriptProbe>,
    #[serde(default)]
    pub failures: Vec<RunFailure>,
}

/// A failure that is no probe's, such as a conflict over `ui/app/src/App.tsx`. It always fails
/// the run.
#[derive(Debug, Deserialize)]
pub(crate) struct RunFailure {
    pub message: String,
    pub fix: String,
}

/// One probe as the script reports it: exactly one of `samples_ms`, `bytes`, `skipped`, `failed`.
#[derive(Debug, Deserialize)]
pub(crate) struct ScriptProbe {
    pub probe: String,
    pub samples_ms: Option<Vec<f64>>,
    pub bytes: Option<f64>,
    pub skipped: Option<String>,
    pub failed: Option<ScriptFailure>,
}

/// A probe the script could not measure: what happened, the command that reproduces it, and the
/// tool's output.
#[derive(Debug, Deserialize)]
pub(crate) struct ScriptFailure {
    pub message: String,
    pub command: String,
    pub output: String,
}

/// Reads the script's stdout: its last non-empty line, as the module doc says.
pub(crate) fn parse_report(stdout: &str) -> Result<ScriptReport, String> {
    let Some(line) = stdout.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Err(format!("{SCRIPT} printed nothing on stdout"));
    };
    serde_json::from_str(line.trim())
        .map_err(|error| format!("{SCRIPT} printed JSON that xtask cannot read: {error}"))
}

/// The script's values for `probe`, in the probe's unit.
fn values(probe: &WebProbe, found: &ScriptProbe) -> Result<Vec<f64>, Failure> {
    let mismatch = || {
        Failure::new(
            format!("{SCRIPT} reported {} in the wrong form", probe.name),
            format!("make {SCRIPT} report {} as the module doc says", probe.name),
        )
    };
    match (probe.unit, &found.samples_ms, found.bytes) {
        (Unit::S, Some(samples), None) => Ok(samples.iter().map(|ms| ms / 1000.0).collect()),
        (Unit::Ms, Some(samples), None) => Ok(samples.clone()),
        (Unit::Kb, None, Some(bytes)) => Ok(vec![bytes / 1000.0]),
        _ => Err(mismatch()),
    }
}

/// The outcome of every web probe from the script's report, in `WEB_PROBES` order.
pub(crate) fn results(report: &ScriptReport, budgets: &[Budget]) -> Vec<ProbeResult> {
    WEB_PROBES
        .iter()
        .map(|probe| ProbeResult {
            name: probe.name,
            unit: probe.unit,
            reported_only: probe.gating == Gating::ReportedOnly,
            outcome: outcome(probe, report, budgets),
        })
        .collect()
}

fn outcome(probe: &WebProbe, report: &ScriptReport, budgets: &[Budget]) -> Outcome {
    let Some(found) = report.probes.iter().find(|found| found.probe == probe.name) else {
        return Outcome::Failed(Failure::new(
            format!("{SCRIPT} reported no {}", probe.name),
            format!("make {SCRIPT} report {}", probe.name),
        ));
    };
    if let Some(failed) = &found.failed {
        return Outcome::Failed(Failure {
            message: one_line(&failed.message),
            fix: format!("run `{}` to see the error, and fix it", failed.command),
            output: Some(failed.output.clone()),
        });
    }
    if let Some(reason) = &found.skipped {
        return Outcome::Skipped(reason.clone());
    }
    let budget = match probe.budget_id {
        Some(id) => match budget_limit(budgets, id, probe.unit) {
            Ok(budget) => Some(budget),
            Err(failure) => return Outcome::Failed(failure),
        },
        None => None,
    };
    match values(probe, found) {
        Ok(values) => Outcome::of_samples(probe.name, values, budget),
        Err(failure) => Outcome::Failed(failure),
    }
}

/// The web half, once it has been measured.
#[derive(Debug)]
pub(crate) struct WebRun {
    pub node: String,
    pub runs: u32,
    pub ci: bool,
    pub results: Vec<ProbeResult>,
    /// The script's failures that are no probe's. Each fails the run.
    pub failures: Vec<Failure>,
}

impl WebRun {
    /// Whether the run passes: no failure outside the probes, and no probe that fails it.
    pub(crate) fn passed(&self) -> bool {
        self.failures.is_empty() && passes(&self.results, self.ci)
    }
}

/// Runs the script and reads its report. Fails before it runs when `ui/app/node_modules` is
/// missing. The script's stderr notes are printed to `diag`. `Err` holds one line per reason the
/// half could not be measured; the reason is printed before it returns.
pub(crate) fn measure(
    root: &Path,
    runs: u32,
    ci: bool,
    budgets: &[Budget],
    diag: &mut Out,
) -> Result<WebRun, Vec<String>> {
    if !root.join("ui/app/node_modules").is_dir() {
        return Err(stop(
            diag,
            "ui/app/node_modules is missing",
            "run `pnpm -C ui install --frozen-lockfile`",
        ));
    }
    let runs_arg = runs.to_string();
    let cmd = Cmd::new(
        Program::Named("node"),
        [SCRIPT, "--runs", &runs_arg],
        SCRIPT_TIMEOUT,
    );
    let rerun = format!("run `{cmd}` to see the error");
    let quiet = match process::run_quiet(&cmd, root) {
        Ok(quiet) => quiet,
        Err(error) if process::is_timeout(&error) => {
            return Err(stop(diag, &error.to_string(), &process::timeout_fix(&cmd)));
        }
        Err(error) => {
            return Err(stop(
                diag,
                &format!("could not run `node`: {error}"),
                &install_node(node_engines(root).as_deref()),
            ));
        }
    };
    let read = read_output(
        &quiet.stdout,
        quiet.success,
        &cmd.to_string(),
        budgets,
        runs,
        ci,
    );
    if !matches!(read, Read::Unreadable(_)) {
        for line in quiet.stderr.lines().filter(|line| !line.trim().is_empty()) {
            diag.note(line);
        }
    }
    match read {
        Read::Run(run) => Ok(run),
        Read::Stopped(failures) => Err(failures
            .iter()
            .flat_map(|failure| stop(diag, &failure.message, &failure.fix))
            .collect()),
        Read::Unreadable(_) if !quiet.success => {
            diag.captured_output(&quiet.stderr);
            Err(stop(diag, &format!("`{cmd}` failed"), &rerun))
        }
        Read::Unreadable(message) => Err(stop(diag, &message, &rerun)),
    }
}

/// The Node range that `engines.node` in `ui/package.json` names, the one place the minimum is
/// written. `None` when the file, its JSON or the field cannot be read.
fn node_engines(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("ui/package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(manifest.get("engines")?.get("node")?.as_str()?.to_owned())
}

/// The fix when `node` cannot run: a Node in the range `engines` (from `node_engines`).
fn install_node(engines: Option<&str>) -> String {
    match engines {
        Some(range) => {
            format!("install a Node that matches {range} (`engines.node` in ui/package.json)")
        }
        None => "install the Node that `engines.node` in ui/package.json names".to_owned(),
    }
}

/// What the script's output comes to.
#[derive(Debug)]
pub(crate) enum Read {
    /// The half was measured.
    Run(WebRun),
    /// The script stopped before it measured, with failures (a conflict left by an interrupted run,
    /// or a Node that is too old).
    Stopped(Vec<Failure>),
    /// The output holds no report xtask can read; the parse error.
    Unreadable(String),
}

/// Reads the script's stdout and exit status. A report is read whatever the exit status, since the
/// script exits non-zero when it reports failures; a non-zero exit with a report but no failures
/// adds one. `cmd` is the command as the fix prints it.
pub(crate) fn read_output(
    stdout: &str,
    success: bool,
    cmd: &str,
    budgets: &[Budget],
    runs: u32,
    ci: bool,
) -> Read {
    let report = match parse_report(stdout) {
        Ok(report) => report,
        Err(message) => return Read::Unreadable(message),
    };
    let mut failures: Vec<Failure> = report
        .failures
        .iter()
        .map(|failure| Failure::new(one_line(&failure.message), failure.fix.clone()))
        .collect();
    if report.probes.is_empty() && !failures.is_empty() {
        return Read::Stopped(failures);
    }
    if !success && failures.is_empty() {
        failures.push(Failure::new(
            format!("`{cmd}` exited non-zero after it reported"),
            format!("run `{cmd}` to see the error"),
        ));
    }
    Read::Run(WebRun {
        results: results(&report, budgets),
        node: report.node,
        runs,
        ci,
        failures,
    })
}
