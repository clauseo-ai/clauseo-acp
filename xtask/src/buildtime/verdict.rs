//! What a probe came to, and whether the run passes: the statistics of the timed runs, a probe's
//! failure, its status against the budget, and the verdict over all of them.

use crate::budgets::{Budget, Gate, Unit};
use serde::Serialize;

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
    pub(crate) fn new(message: impl Into<String>, fix: impl Into<String>) -> Failure {
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
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProbeStatus::Skipped => "skipped",
            ProbeStatus::Ok => "ok",
            ProbeStatus::Over => "over",
            ProbeStatus::Failed => "failed",
        }
    }
}

/// What one probe came to. A measured probe carries its budget row's gate, and a warning when its
/// build was not incremental.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    Skipped(&'static str),
    Failed(Failure),
    Measured {
        stats: Stats,
        budget_id: &'static str,
        budget_s: f64,
        gate: Gate,
        warning: Option<String>,
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

    /// Whether this probe fails the run: a failure always does, and an over-budget probe does when
    /// its gate says so for this machine.
    pub(crate) fn fails_run(&self, ci: bool) -> bool {
        match self {
            Outcome::Skipped(_) => false,
            Outcome::Failed(_) => true,
            Outcome::Measured { gate, .. } => {
                self.status() == ProbeStatus::Over && over_budget_fails(*gate, ci)
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

/// One probe's name and outcome.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeResult {
    pub name: &'static str,
    pub outcome: Outcome,
}

/// A budget row's limit in seconds, and its gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Limit {
    pub seconds: f64,
    pub gate: Gate,
}

/// The limit of the budget row `id`, in seconds, and its gate. A missing row, or a unit other than
/// `s` or `ms`, fails the probe.
pub(crate) fn budget_limit(budgets: &[Budget], id: &str) -> Result<Limit, Failure> {
    let Some(budget) = budgets.iter().find(|budget| budget.id == id) else {
        return Err(Failure::new(
            format!("no budget row {id} in budgets.toml"),
            format!("add a [[budget]] row with id = \"{id}\" to budgets.toml"),
        ));
    };
    let seconds = match budget.unit {
        Unit::S => budget.limit,
        Unit::Ms => budget.limit / 1000.0,
        other => {
            return Err(Failure::new(
                format!(
                    "budget {id} is in {}, and buildtime compares s or ms",
                    other.label()
                ),
                format!("set unit = \"s\" or \"ms\" on budget {id}"),
            ));
        }
    };
    Ok(Limit {
        seconds,
        gate: budget.gate,
    })
}

/// Whether the run passes: no probe fails it.
pub(crate) fn passes(results: &[ProbeResult], ci: bool) -> bool {
    !results.iter().any(|result| result.outcome.fails_run(ci))
}
