//! The web half: `node ui/app/scripts/buildtime.mjs --runs N` measures the frontend's edit loop and
//! prints one JSON object; this module reads it, computes the statistics with `verdict::stats`,
//! compares them with the rows of `budgets.toml`, and shows the result.
//!
//! The script times `tsc --noEmit`, `oxlint --type-aware` and `vite build` (one warm-up, then N
//! runs each), sizes the initial JS of that build, starts the Vite dev server on a free port
//! (`dev_start`), and times N edits of `src/App.tsx` until headless Chrome shows them (`hmr`). It
//! puts App.tsx back to its exact bytes, and stops only the Vite process and the browser it started.
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
}

impl WebRun {
    /// Whether no web probe fails the run.
    pub(crate) fn passed(&self) -> bool {
        passes(&self.results, self.ci)
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
    if !quiet.success {
        diag.captured_output(&quiet.stderr);
        return Err(stop(diag, &format!("`{cmd}` failed"), &rerun));
    }
    for line in quiet.stderr.lines().filter(|line| !line.trim().is_empty()) {
        diag.note(line);
    }
    let report = parse_report(&quiet.stdout).map_err(|message| stop(diag, &message, &rerun))?;
    Ok(WebRun {
        results: results(&report, budgets),
        node: report.node,
        runs,
        ci,
    })
}
