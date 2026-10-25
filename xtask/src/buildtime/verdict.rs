//! What a probe came to, and whether the run passes: the statistics of the timed runs, a probe's
//! failure, its status against the budget, and the verdict over all of them. Both halves share
//! these types: a Rust probe measures seconds, a web probe seconds, milliseconds or kilobytes.

use crate::budgets::{Budget, Gate, Unit};
use crate::write::GuardFailure;
use serde::Serialize;

/// The median, minimum and maximum of the timed runs. Unit-free: the values are in whatever unit
/// the probe measures (`ProbeResult::unit`).
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
    pub(crate) fn new(message: impl Into<String>, fix: impl Into<String>) -> Failure {
        Failure {
            message: message.into(),
            fix: fix.into(),
            output: None,
        }
    }
}

/// The tracked-file guard's failure, as the probe that used it fails.
impl From<GuardFailure> for Failure {
    fn from(failure: GuardFailure) -> Failure {
        Failure::new(failure.message, failure.fix)
    }
}

/// The status of a probe, as the table and the JSON print it. `Reported` is a measured probe with
/// no budget row (the web half's `dev_start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProbeStatus {
    Skipped,
    Ok,
    Over,
    Failed,
    Reported,
}

impl ProbeStatus {
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProbeStatus::Skipped => "skipped",
            ProbeStatus::Ok => "ok",
            ProbeStatus::Over => "over",
            ProbeStatus::Failed => "failed",
            ProbeStatus::Reported => "reported",
        }
    }
}

/// What one probe came to. A measured probe carries its samples and their stats in the probe's
/// unit, its budget (none for a probe without a row, which is only reported), and a warning when
/// its build was not incremental.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    Skipped(String),
    Failed(Failure),
    Measured {
        samples: Vec<f64>,
        stats: Stats,
        budget: Option<Limit>,
        warning: Option<String>,
    },
}

impl Outcome {
    /// The outcome of `samples` against `budget`. No samples fails the probe.
    pub(crate) fn of_samples(name: &str, samples: Vec<f64>, budget: Option<Limit>) -> Outcome {
        match stats(&samples) {
            Some(stats) => Outcome::Measured {
                samples,
                stats,
                budget,
                warning: None,
            },
            None => Outcome::Failed(Failure::new(
                format!("no timed runs for {name}"),
                "run `cargo xtask buildtime` with --runs 1 or more",
            )),
        }
    }

    /// `ok` when the median is within the budget, `over` when it is not, `reported` with no
    /// budget.
    pub(crate) fn status(&self) -> ProbeStatus {
        match self {
            Outcome::Skipped(_) => ProbeStatus::Skipped,
            Outcome::Failed(_) => ProbeStatus::Failed,
            Outcome::Measured { budget: None, .. } => ProbeStatus::Reported,
            Outcome::Measured {
                stats,
                budget: Some(budget),
                ..
            } => {
                if stats.median <= budget.value {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Over
                }
            }
        }
    }
}

/// One probe: its name, the unit of its values, whether it is reported only, and its outcome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeResult {
    pub name: &'static str,
    pub unit: Unit,
    /// Over budget, the probe never fails the run. It still fails it when it fails: a probe that
    /// produced no number fails the run, reported only or not, in CI too.
    pub reported_only: bool,
    pub outcome: Outcome,
}

impl ProbeResult {
    pub(crate) fn status(&self) -> ProbeStatus {
        self.outcome.status()
    }

    /// Whether this probe fails the run. A failure always does, on every machine and for a
    /// reported-only probe too: it produced no number. A skipped probe never does: it does not
    /// apply here, or it follows a failure that already fails the run. A web skip that nothing
    /// explains reaches here as a failure, since `web::results` makes it one. An over-budget probe
    /// does when it is not reported only and its gate says so for this machine.
    pub(crate) fn fails_run(&self, ci: bool) -> bool {
        match &self.outcome {
            Outcome::Skipped(_) => false,
            Outcome::Failed(_) => true,
            Outcome::Measured { budget, .. } => {
                !self.reported_only
                    && self.status() == ProbeStatus::Over
                    && budget.is_some_and(|budget| over_budget_fails(budget.gate, ci))
            }
        }
    }
}

/// Whether an over-budget probe fails the run. `local` fails here and is only reported in CI, `pr`
/// and `release` fail everywhere, and `alert` is only reported.
pub(crate) fn over_budget_fails(gate: Gate, ci: bool) -> bool {
    match gate {
        Gate::Local => !ci,
        Gate::Alert => false,
        Gate::Pr | Gate::Release => true,
    }
}

/// A budget row's id, its limit in the probe's unit, and its gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Limit {
    pub id: &'static str,
    pub value: f64,
    pub gate: Gate,
}

/// The factor from a budget row's unit to a probe's unit, or `None` when they do not convert.
fn factor(row: Unit, probe: Unit) -> Option<f64> {
    match (row, probe) {
        _ if row == probe => Some(1.0),
        (Unit::Ms, Unit::S) => Some(0.001),
        (Unit::S, Unit::Ms) => Some(1000.0),
        _ => None,
    }
}

/// The budget row `id` with its limit in `unit`, the unit the probe measures, and its gate. A
/// missing row, or a row in a unit that does not convert to `unit`, fails the probe.
pub(crate) fn budget_limit(
    budgets: &[Budget],
    id: &'static str,
    unit: Unit,
) -> Result<Limit, Failure> {
    let Some(row) = budgets.iter().find(|budget| budget.id == id) else {
        return Err(Failure::new(
            format!("no budget row {id} in budgets.toml"),
            format!("add a [[budget]] row with id = \"{id}\" to budgets.toml"),
        ));
    };
    let Some(factor) = factor(row.unit, unit) else {
        return Err(Failure::new(
            format!(
                "budget {id} is in {}, and this probe measures {}",
                row.unit.label(),
                unit.label()
            ),
            format!("set unit = \"{}\" on budget {id}", unit.label()),
        ));
    };
    Ok(Limit {
        id,
        value: row.limit * factor,
        gate: row.gate,
    })
}

/// Whether the run passes: no probe fails it.
pub(crate) fn passes(results: &[ProbeResult], ci: bool) -> bool {
    !results.iter().any(|result| result.fails_run(ci))
}
