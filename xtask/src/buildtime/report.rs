//! How a run is shown: the text table, the failures, and the JSON object, the same for both
//! halves. Stdout is the table or the object; diagnostics go through the printer that
//! `buildtime::run` picks.

use super::RustRun;
use super::verdict::{Outcome, ProbeResult, ProbeStatus};
use super::web::WebRun;
use crate::budgets::Unit;
use crate::output::Printer;
use serde::Serialize;
use std::io::Write;

/// The result table: a header, then one line per probe, each value in its probe's unit. Measured
/// probes align their columns, and one without a budget prints `-` for it; a skipped or failed
/// probe prints its state in place of the columns.
pub(super) fn table_lines(results: &[ProbeResult]) -> Vec<String> {
    let rows: Vec<(&str, Result<[String; 5], String>)> = results
        .iter()
        .map(|result| (result.name, cells(result)))
        .collect();
    aligned_table(&rows)
}

/// The table of `rows`: a header, then one line per row. Each row is its name and either its five
/// cells, aligned in columns, or the text it prints in place of them.
fn aligned_table(rows: &[(&str, Result<[String; 5], String>)]) -> Vec<String> {
    const NAME_HEADER: &str = "probe";
    const COLUMNS: [&str; 5] = ["median", "min", "max", "budget", "status"];
    let name_width = rows
        .iter()
        .map(|(name, _)| name.len())
        .chain([NAME_HEADER.len()])
        .max()
        .unwrap_or(0);
    let mut widths = COLUMNS.map(str::len);
    for (_, row) in rows {
        if let Ok(row) = row {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.len());
            }
        }
    }
    let line =
        |name: &str, body: &str| format!("{name:<name_width$}  {body}").trim_end().to_owned();
    let mut lines = vec![line(NAME_HEADER, &join_cells(&COLUMNS, &widths))];
    for (name, row) in rows {
        lines.push(match row {
            Ok(row) => line(name, &join_cells(row, &widths)),
            Err(text) => line(name, text),
        });
    }
    lines
}

/// The five columns of a measured probe, or the text a skipped or failed probe prints instead.
fn cells(result: &ProbeResult) -> Result<[String; 5], String> {
    let unit = result.unit;
    match &result.outcome {
        Outcome::Measured { stats, budget, .. } => Ok([
            show(unit, stats.median),
            show(unit, stats.min),
            show(unit, stats.max),
            budget.map_or_else(|| "-".to_owned(), |budget| show(unit, budget.value)),
            result.status().word().to_owned(),
        ]),
        Outcome::Skipped(reason) => Err(format!("skipped: {reason}")),
        Outcome::Failed(_) => Err("failed (see below)".to_owned()),
    }
}

/// `value` in `unit`, for the table. Kilobytes are of 1000 bytes, as in Vite's own `kB` output.
fn show(unit: Unit, value: f64) -> String {
    match unit {
        Unit::S => format!("{value:.2}s"),
        Unit::Ms => format!("{value:.0}ms"),
        Unit::Kb => format!("{value:.1}KB"),
        Unit::Mb => format!("{value:.1}MB"),
        Unit::Count => format!("{value}"),
    }
}

/// Joins `cells` left-aligned to `widths`, two spaces apart.
fn join_cells(cells: &[impl AsRef<str>], widths: &[usize]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{:<width$}", cell.as_ref()))
        .collect::<Vec<_>>()
        .join("  ")
}

/// The warning under the table: one line for each measured probe whose build was not incremental.
pub(super) fn warning_lines(results: &[ProbeResult]) -> Vec<String> {
    results
        .iter()
        .filter_map(|result| match &result.outcome {
            Outcome::Measured {
                warning: Some(warning),
                ..
            } => Some(warning.clone()),
            _ => None,
        })
        .collect()
}

/// The output of each failed probe: the tool's output when it ran, then the failure and its fix.
/// A reported-only probe's failure says that the run still passes.
pub(super) fn print_failures<W: Write>(results: &[ProbeResult], diag: &mut Printer<W>) {
    for result in results {
        if let Outcome::Failed(failure) = &result.outcome {
            if let Some(output) = &failure.output {
                diag.captured_output(output);
            }
            if result.reported_only {
                let message = format!("{} ({REPORTED_FAILURE})", failure.message);
                diag.failure(&message, &failure.fix);
            } else {
                diag.failure(&failure.message, &failure.fix);
            }
        }
    }
}

/// Added to the failure of a reported-only probe.
pub(super) const REPORTED_FAILURE: &str = "reported only; the run still passes";

/// One probe in the JSON report, the same shape for both halves. Values are in `unit`; absent
/// fields are left out.
#[derive(Debug, Serialize)]
pub(super) struct ProbeJson {
    probe: &'static str,
    status: ProbeStatus,
    unit: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    median: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    /// Every timed run, in the order it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    samples: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_id: Option<&'static str>,
    /// True when the probe never fails the run, whatever it came to.
    reported_only: bool,
    /// Why a probe was skipped or failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// The not-incremental warning of a measured Rust probe. Absent when the build was incremental.
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

pub(super) fn probe_json(result: &ProbeResult) -> ProbeJson {
    let mut json = ProbeJson {
        probe: result.name,
        status: result.status(),
        unit: result.unit.label(),
        median: None,
        min: None,
        max: None,
        samples: None,
        budget: None,
        budget_id: None,
        reported_only: result.reported_only,
        reason: None,
        warning: None,
    };
    match &result.outcome {
        Outcome::Skipped(reason) => json.reason = Some(reason.clone()),
        Outcome::Failed(failure) => json.reason = Some(failure.message.clone()),
        Outcome::Measured {
            samples,
            stats,
            budget,
            warning,
        } => {
            json.median = Some(stats.median);
            json.min = Some(stats.min);
            json.max = Some(stats.max);
            json.samples = Some(samples.clone());
            json.budget = budget.map(|budget| budget.value);
            json.budget_id = budget.map(|budget| budget.id);
            json.warning.clone_from(warning);
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
        probes: Vec<ProbeJson>,
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
                probes: run.results.iter().map(probe_json).collect(),
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

/// The JSON object of a run: the Rust half, and `web` when both halves ran. `runs` counts the timed
/// runs per probe, after the warm-up. `passed` is the verdict of the run.
#[derive(Serialize)]
struct Report<'a> {
    rustc: &'a str,
    profile: &'static str,
    /// The expected setting; each probe's warm-up decides what the build really used.
    incremental: bool,
    ci: bool,
    runs: u32,
    passed: bool,
    probes: Vec<ProbeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    web: Option<&'a WebJson>,
}

/// The JSON object of `web` alone: the web half, and the verdict of the run.
#[derive(Serialize)]
struct WebOnly<'a> {
    web: &'a WebJson,
    passed: bool,
}

/// The JSON object of a run that stopped before it measured: `passed` is false, and `errors` holds
/// one line per reason.
#[derive(Serialize)]
struct Errors<'a> {
    passed: bool,
    errors: &'a [String],
}

pub(super) fn json_line<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}

/// The JSON object of a measured run. `web` adds the web half, which only `all` has. `passed` is
/// the verdict over both halves.
pub(super) fn json_object(run: &RustRun, passed: bool, web: Option<&WebJson>) -> String {
    let report = Report {
        rustc: &run.rustc,
        profile: "dev",
        incremental: run.incremental.on,
        ci: run.ci,
        runs: run.runs,
        passed,
        probes: run.results.iter().map(probe_json).collect(),
        web,
    };
    json_line(&report)
}

/// The JSON object of a run that stopped before it measured: one line per error.
pub(super) fn error_json(errors: &[String]) -> String {
    json_line(&Errors {
        passed: false,
        errors,
    })
}

/// The JSON object of `web` alone: the web half under `web`, measured or stopped, and its verdict.
pub(super) fn web_json(web: &WebJson) -> String {
    json_line(&WebOnly {
        web,
        passed: web.passed(),
    })
}
