//! How a run is shown: the text table, the failures, and the JSON object. Stdout is the table
//! or the object; diagnostics go through the printer that `buildtime::run` picks.

use super::{Out, Outcome, ProbeResult, ProbeStatus, RustRun, WEB_MESSAGE};
use serde::Serialize;

/// The result table: a header, then one line per probe. Measured probes align their columns; a
/// skipped or failed probe prints its state in place of the columns.
pub(super) fn table_lines(results: &[ProbeResult]) -> Vec<String> {
    const NAME_HEADER: &str = "probe";
    const COLUMNS: [&str; 5] = ["median", "min", "max", "budget", "status"];
    let name_width = results
        .iter()
        .map(|result| result.name.len())
        .chain([NAME_HEADER.len()])
        .max()
        .unwrap_or(0);
    let rows: Vec<(&str, Result<[String; 5], String>)> = results
        .iter()
        .map(|result| (result.name, cells(&result.outcome)))
        .collect();
    let mut widths = COLUMNS.map(str::len);
    for (_, row) in &rows {
        if let Ok(row) = row {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.len());
            }
        }
    }
    let line =
        |name: &str, body: &str| format!("{name:<name_width$}  {body}").trim_end().to_owned();
    let mut lines = vec![line(NAME_HEADER, &join_cells(&COLUMNS, &widths))];
    for (name, row) in &rows {
        lines.push(match row {
            Ok(row) => line(name, &join_cells(row, &widths)),
            Err(text) => line(name, text),
        });
    }
    lines
}

/// The five columns of a measured probe, or the text a skipped or failed probe prints instead.
fn cells(outcome: &Outcome) -> Result<[String; 5], String> {
    match outcome {
        Outcome::Measured {
            stats, budget_s, ..
        } => Ok([
            secs(stats.median),
            secs(stats.min),
            secs(stats.max),
            secs(*budget_s),
            outcome.status().word().to_owned(),
        ]),
        Outcome::Skipped(reason) => Err(format!("skipped: {reason}")),
        Outcome::Failed(_) => Err("failed (see below)".to_owned()),
    }
}

fn secs(value: f64) -> String {
    format!("{value:.2}s")
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

/// The output of each failed probe: cargo's output when it ran, then the failure and its fix.
pub(super) fn print_failures(results: &[ProbeResult], diag: &mut Out) {
    let failures = results.iter().filter_map(|result| match &result.outcome {
        Outcome::Failed(failure) => Some(failure),
        _ => None,
    });
    for failure in failures {
        if let Some(output) = &failure.output {
            diag.captured_output(output);
        }
        diag.failure(&failure.message, &failure.fix);
    }
}

/// One probe in the JSON report. Absent fields are left out.
#[derive(Serialize)]
pub(super) struct ProbeJson {
    probe: &'static str,
    status: ProbeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    median_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_id: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_s: Option<f64>,
}

pub(super) fn probe_json(result: &ProbeResult) -> ProbeJson {
    let mut json = ProbeJson {
        probe: result.name,
        status: result.outcome.status(),
        reason: None,
        median_s: None,
        min_s: None,
        max_s: None,
        budget_id: None,
        budget_s: None,
    };
    match &result.outcome {
        Outcome::Skipped(reason) => json.reason = Some((*reason).to_owned()),
        Outcome::Failed(failure) => json.reason = Some(failure.message.clone()),
        Outcome::Measured {
            stats,
            budget_id,
            budget_s,
            ..
        } => {
            json.median_s = Some(stats.median);
            json.min_s = Some(stats.min);
            json.max_s = Some(stats.max);
            json.budget_id = Some(budget_id);
            json.budget_s = Some(*budget_s);
        }
    }
    json
}

/// The JSON object of a run: the Rust half, and `web` when both halves ran. `runs` counts the timed
/// runs per probe, after the warm-up. `passed` is the verdict of the run.
#[derive(Serialize)]
struct Report<'a> {
    rustc: &'a str,
    profile: &'static str,
    incremental: bool,
    ci: bool,
    runs: u32,
    passed: bool,
    probes: Vec<ProbeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    web: Option<&'static str>,
}

/// The JSON object of `web` alone. The web half is not built yet, so it does not fail.
#[derive(Serialize)]
pub(super) struct WebOnly {
    pub(super) web: &'static str,
    pub(super) passed: bool,
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

/// The JSON object of a measured run. `web` adds the web field, which only `all` prints.
pub(super) fn json_object(run: &RustRun, passed: bool, web: bool) -> String {
    let report = Report {
        rustc: &run.rustc,
        profile: "dev",
        incremental: run.incremental.on,
        ci: run.ci,
        runs: run.runs,
        passed,
        probes: run.results.iter().map(probe_json).collect(),
        web: web.then_some(WEB_MESSAGE),
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

/// The JSON object of `web` alone.
pub(super) fn web_json() -> String {
    json_line(&WebOnly {
        web: WEB_MESSAGE,
        passed: true,
    })
}
