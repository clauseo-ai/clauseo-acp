//! The web half: `node ui/app/scripts/buildtime.mjs --runs N` measures the frontend's edit loop and
//! prints one JSON object; this module reads it, computes the statistics with `verdict::stats`,
//! compares them with the rows of `budgets.toml`, and shows the result.
//!
//! The script times `tsc --noEmit`, `oxlint --type-aware` and `vite build` (one warm-up, then N
//! runs each), sizes the initial JS of that build, starts the Vite dev server on a free port
//! (`dev_start`), and times N edits of `src/App.tsx` until headless Chrome shows them (`hmr`). It
//! stops only the processes it started.
//!
//! App.tsx follows the rules of `probe.rs`: a copy of the original in
//! `target/xtask/buildtime/ui/app/src/App.tsx` before the first edit, every write through a
//! temporary file and a rename, and the original put back only over bytes the script wrote, also
//! on SIGINT, SIGTERM and SIGHUP. The leftover rule lives in the script, in one place
//! (`recoverApp` in `ui/app/scripts/buildtime-lib.mjs`): at the start of every run it puts the
//! copy back only when App.tsx differs from it by one `data-buildtime` attribute at `ROOT_TAG`.
//! `probe::restore_leftovers` skips `target/xtask/buildtime/ui/`. A developer's edit is never
//! overwritten: both files stay, and the conflict fails the run. The lock (`lock.rs`) covers the
//! copy when xtask runs the script.
//!
//! Gating: `tsc`, `oxlint_type_aware` and `vite_build` follow their row's gate like the Rust probes,
//! and fail the run when they fail (the tool exits non-zero). `initial_js`, `dev_start` and `hmr`
//! are reported only: over budget or failed, they show in the table, a failure prints its lines,
//! and the run still passes. A skipped probe (no Chrome) never fails the run.

#[cfg(test)]
mod tests;

use super::verdict::{Failure, Outcome, ProbeResult, budget_limit, passes};
use super::{Out, one_line, stop};
use crate::budgets::{Budget, Unit};
use crate::process::{self, Cmd, Program};
use serde::Deserialize;
use std::path::Path;

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

/// Reads the script's stdout.
pub(crate) fn parse_report(stdout: &str) -> Result<ScriptReport, String> {
    serde_json::from_str(stdout.trim())
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
    let cmd = Cmd::new(Program::Named("node"), [SCRIPT, "--runs", &runs_arg]);
    let rerun = format!("run `{cmd}` to see the error");
    let quiet = match process::run_quiet(&cmd, root) {
        Ok(quiet) => quiet,
        Err(error) => {
            return Err(stop(
                diag,
                &format!("could not run `node`: {error}"),
                "install Node 24 or later",
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

/// What the script's output comes to.
#[derive(Debug)]
pub(crate) enum Read {
    /// The half was measured.
    Run(WebRun),
    /// The script stopped before it measured, with failures (a conflict left by an interrupted run).
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
