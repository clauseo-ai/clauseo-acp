//! The web half: `node ui/app/scripts/buildtime.mjs --runs N` measures the frontend's edit loop and
//! prints one JSON object; this module reads it, computes the statistics with `verdict::stats`,
//! compares them with the rows of `budgets.toml`, and shows the result.
//!
//! The script times `tsc --noEmit`, `oxlint --type-aware` and `vite build` (one warm-up, then N
//! runs each), sizes the initial JS of that build, starts the Vite dev server on a free port
//! (`dev_start`), and times N edits of `src/App.tsx` until headless Chrome shows them (`hmr`). It
//! puts App.tsx back to its exact bytes, and stops only the Vite process and the browser it started.
//!
//! Gating: `tsc`, `oxlint_type_aware` and `vite_build` follow their row's gate like the Rust probes.
//! `initial_js` is recorded only until its PR gate starts, and `hmr` and `dev_start` are reported and
//! never gated. A probe that fails (a tool exits non-zero, the dev server does not start) fails the
//! run; a skipped one (no Chrome) does not.

#[cfg(test)]
mod tests;

use super::verdict::{Failure, ProbeStatus, Stats, over_budget_fails, stats};
use super::{Out, one_line, stop};
use crate::budgets::{Budget, Gate, Unit};
use crate::process::{self, Cmd, Program};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The script, from the repo root.
pub(crate) const SCRIPT: &str = "ui/app/scripts/buildtime.mjs";

/// The unit a web probe is shown and budgeted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebUnit {
    /// Seconds, from the script's milliseconds.
    S,
    /// Milliseconds, as the script measures them.
    Ms,
    /// Kilobytes of 1000 bytes, as in Vite's own `kB` output, from the script's bytes.
    Kb,
}

impl WebUnit {
    /// The unit as the table and the JSON write it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            WebUnit::S => "s",
            WebUnit::Ms => "ms",
            WebUnit::Kb => "KB",
        }
    }

    /// `value` in this unit, for the table.
    fn show(self, value: f64) -> String {
        match self {
            WebUnit::S => format!("{value:.2}s"),
            WebUnit::Ms => format!("{value:.0}ms"),
            WebUnit::Kb => format!("{value:.1}KB"),
        }
    }

    /// The factor from a budget row's unit to this one, or `None` when they do not convert.
    fn factor_from(self, unit: Unit) -> Option<f64> {
        match (self, unit) {
            (WebUnit::S, Unit::S) | (WebUnit::Ms, Unit::Ms) | (WebUnit::Kb, Unit::Kb) => Some(1.0),
            (WebUnit::S, Unit::Ms) => Some(0.001),
            (WebUnit::Ms, Unit::S) => Some(1000.0),
            _ => None,
        }
    }
}

/// How an over-budget probe is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gating {
    /// Its budget row's gate decides, as for the Rust probes.
    Row,
    /// Reported, never fails the run.
    ReportedOnly,
}

/// One web probe: the script's name for it, its budget row, its unit and its gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WebProbe {
    pub name: &'static str,
    pub budget_id: Option<&'static str>,
    pub unit: WebUnit,
    pub gating: Gating,
}

/// The six probes, in the order the script runs them and the table prints them.
pub(crate) const WEB_PROBES: &[WebProbe] = &[
    WebProbe {
        name: "tsc",
        budget_id: Some("tsc_noemit"),
        unit: WebUnit::S,
        gating: Gating::Row,
    },
    WebProbe {
        name: "oxlint_type_aware",
        budget_id: Some("oxlint_type_aware"),
        unit: WebUnit::S,
        gating: Gating::Row,
    },
    WebProbe {
        name: "vite_build",
        budget_id: Some("vite_build_warm"),
        unit: WebUnit::S,
        gating: Gating::Row,
    },
    // Recorded only: its PR gate starts later.
    WebProbe {
        name: "initial_js",
        budget_id: Some("initial_js"),
        unit: WebUnit::Kb,
        gating: Gating::ReportedOnly,
    },
    WebProbe {
        name: "dev_start",
        budget_id: None,
        unit: WebUnit::Ms,
        gating: Gating::ReportedOnly,
    },
    WebProbe {
        name: "hmr",
        budget_id: Some("hot_reload_component_edit"),
        unit: WebUnit::Ms,
        gating: Gating::ReportedOnly,
    },
];

/// The line under the web table that names the probes that never fail the run on their numbers.
pub(crate) const REPORTED_LINE: &str =
    "reported only: initial_js (its PR gate starts later), dev_start, hmr";

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

/// A web probe's budget row, its limit in the probe's unit, and the gate that judges it here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WebBudget {
    pub id: &'static str,
    pub limit: f64,
    pub gate: Gate,
}

/// What one web probe came to. A measured probe without a budget row is only reported.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WebOutcome {
    Skipped(String),
    Failed(Failure),
    Measured {
        stats: Stats,
        budget: Option<WebBudget>,
    },
}

/// One web probe and its outcome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WebResult {
    pub probe: WebProbe,
    pub outcome: WebOutcome,
}

impl WebResult {
    /// `ok` or `over` against the budget, `reported` with none.
    pub(crate) fn status(&self) -> ProbeStatus {
        match &self.outcome {
            WebOutcome::Skipped(_) => ProbeStatus::Skipped,
            WebOutcome::Failed(_) => ProbeStatus::Failed,
            WebOutcome::Measured { budget: None, .. } => ProbeStatus::Reported,
            WebOutcome::Measured {
                stats,
                budget: Some(budget),
            } => {
                if stats.median <= budget.limit {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Over
                }
            }
        }
    }

    /// Whether being over budget fails the run here.
    pub(crate) fn gated(&self, ci: bool) -> bool {
        match &self.outcome {
            WebOutcome::Measured {
                budget: Some(budget),
                ..
            } => over_budget_fails(budget.gate, ci),
            _ => false,
        }
    }

    /// Whether this probe fails the run: a failure always does, and an over-budget probe does
    /// when it is gated here.
    pub(crate) fn fails_run(&self, ci: bool) -> bool {
        match self.status() {
            ProbeStatus::Failed => true,
            ProbeStatus::Over => self.gated(ci),
            _ => false,
        }
    }
}

/// The budget row `id` with its limit in `unit`. A reported-only probe gets the `alert` gate,
/// whatever its row says.
fn web_budget(
    budgets: &[Budget],
    id: &'static str,
    unit: WebUnit,
    gating: Gating,
) -> Result<WebBudget, Failure> {
    let Some(row) = budgets.iter().find(|budget| budget.id == id) else {
        return Err(Failure::new(
            format!("no budget row {id} in budgets.toml"),
            format!("add a [[budget]] row with id = \"{id}\" to budgets.toml"),
        ));
    };
    let Some(factor) = unit.factor_from(row.unit) else {
        return Err(Failure::new(
            format!(
                "budget {id} is in {}, and this probe measures {}",
                row.unit.label(),
                unit.label()
            ),
            format!("set unit = \"{}\" on budget {id}", unit.label()),
        ));
    };
    let gate = match gating {
        Gating::Row => row.gate,
        Gating::ReportedOnly => Gate::Alert,
    };
    Ok(WebBudget {
        id,
        limit: row.limit * factor,
        gate,
    })
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
        (WebUnit::S, Some(samples), None) => Ok(samples.iter().map(|ms| ms / 1000.0).collect()),
        (WebUnit::Ms, Some(samples), None) => Ok(samples.clone()),
        (WebUnit::Kb, None, Some(bytes)) => Ok(vec![bytes / 1000.0]),
        _ => Err(mismatch()),
    }
}

/// The outcome of every web probe from the script's report, in `WEB_PROBES` order.
pub(crate) fn results(report: &ScriptReport, budgets: &[Budget]) -> Vec<WebResult> {
    WEB_PROBES
        .iter()
        .map(|probe| WebResult {
            probe: *probe,
            outcome: outcome(probe, report, budgets),
        })
        .collect()
}

fn outcome(probe: &WebProbe, report: &ScriptReport, budgets: &[Budget]) -> WebOutcome {
    let Some(found) = report.probes.iter().find(|found| found.probe == probe.name) else {
        return WebOutcome::Failed(Failure::new(
            format!("{SCRIPT} reported no {}", probe.name),
            format!("make {SCRIPT} report {}", probe.name),
        ));
    };
    if let Some(failed) = &found.failed {
        return WebOutcome::Failed(Failure {
            message: one_line(&failed.message),
            fix: format!("run `{}` to see the error, and fix it", failed.command),
            output: Some(failed.output.clone()),
        });
    }
    if let Some(reason) = &found.skipped {
        return WebOutcome::Skipped(reason.clone());
    }
    let budget = match probe.budget_id {
        Some(id) => match web_budget(budgets, id, probe.unit, probe.gating) {
            Ok(budget) => Some(budget),
            Err(failure) => return WebOutcome::Failed(failure),
        },
        None => None,
    };
    match values(probe, found).map(|values| stats(&values)) {
        Ok(Some(stats)) => WebOutcome::Measured { stats, budget },
        Ok(None) => WebOutcome::Failed(Failure::new(
            format!("{SCRIPT} reported no timed runs for {}", probe.name),
            "run `cargo xtask buildtime web` with --runs 1 or more",
        )),
        Err(failure) => WebOutcome::Failed(failure),
    }
}

/// The web half, once it has been measured.
#[derive(Debug)]
pub(crate) struct WebRun {
    pub node: String,
    pub runs: u32,
    pub ci: bool,
    pub results: Vec<WebResult>,
}

impl WebRun {
    /// Whether no web probe fails the run.
    pub(crate) fn passed(&self) -> bool {
        !self.results.iter().any(|result| result.fails_run(self.ci))
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

/// The web table, in the format of the Rust half's: a header, then one line per probe. A probe
/// without a budget prints `-` in the budget column.
pub(crate) fn table_lines(results: &[WebResult]) -> Vec<String> {
    let rows: Vec<(&str, Result<[String; 5], String>)> = results
        .iter()
        .map(|result| (result.probe.name, cells(result)))
        .collect();
    super::report::aligned_table(&rows)
}

fn cells(result: &WebResult) -> Result<[String; 5], String> {
    let unit = result.probe.unit;
    match &result.outcome {
        WebOutcome::Measured { stats, budget } => Ok([
            unit.show(stats.median),
            unit.show(stats.min),
            unit.show(stats.max),
            budget.map_or_else(|| "-".to_owned(), |budget| unit.show(budget.limit)),
            result.status().word().to_owned(),
        ]),
        WebOutcome::Skipped(reason) => Err(format!("skipped: {reason}")),
        WebOutcome::Failed(_) => Err("failed (see below)".to_owned()),
    }
}

/// One web probe in the JSON report. Values are in `unit`; absent fields are left out.
#[derive(Debug, Serialize)]
pub(crate) struct WebProbeJson {
    probe: &'static str,
    status: ProbeStatus,
    unit: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    median: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_id: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget: Option<f64>,
    /// True when being over budget would not fail this run.
    reported_only: bool,
}

fn probe_json(result: &WebResult, ci: bool) -> WebProbeJson {
    let mut json = WebProbeJson {
        probe: result.probe.name,
        status: result.status(),
        unit: result.probe.unit.label(),
        reason: None,
        median: None,
        min: None,
        max: None,
        budget_id: None,
        budget: None,
        reported_only: !result.gated(ci),
    };
    match &result.outcome {
        WebOutcome::Skipped(reason) => json.reason = Some(reason.clone()),
        WebOutcome::Failed(failure) => json.reason = Some(failure.message.clone()),
        WebOutcome::Measured { stats, budget } => {
            json.median = Some(stats.median);
            json.min = Some(stats.min);
            json.max = Some(stats.max);
            json.budget_id = budget.map(|budget| budget.id);
            json.budget = budget.map(|budget| budget.limit);
        }
    }
    json
}

/// The web half in the JSON report: measured, or stopped before it measured.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum WebJson {
    Measured {
        node: String,
        ci: bool,
        runs: u32,
        passed: bool,
        probes: Vec<WebProbeJson>,
    },
    Stopped {
        passed: bool,
        errors: Vec<String>,
    },
}

impl WebJson {
    /// The JSON of a web half that measured, or of one that stopped with `errors`.
    pub(crate) fn of(run: &Result<WebRun, Vec<String>>) -> WebJson {
        match run {
            Ok(run) => WebJson::Measured {
                node: run.node.clone(),
                ci: run.ci,
                runs: run.runs,
                passed: run.passed(),
                probes: run
                    .results
                    .iter()
                    .map(|result| probe_json(result, run.ci))
                    .collect(),
            },
            Err(errors) => WebJson::Stopped {
                passed: false,
                errors: errors.clone(),
            },
        }
    }

    /// The web half's verdict.
    pub(crate) fn passed(&self) -> bool {
        match self {
            WebJson::Measured { passed, .. } | WebJson::Stopped { passed, .. } => *passed,
        }
    }
}
