//! Reads cargo-mutants' `outcomes.json` and judges it.
//!
//! The fields read, as cargo-mutants 27.1.0 writes them (`LabOutcome` and `ScenarioOutcome` in its
//! `src/outcome.rs`, `Mutant` in `src/mutant.rs`); serde skips the rest:
//! - `end_time`: set when the run finished, `null` while it runs.
//! - `outcomes[]`, one per scenario:
//!   - `scenario`: `"Baseline"` (the unmutated tree), or `{"Mutant": {...}}` with the mutant's
//!     `name` (`file:line:column: mutation`), `file` (repo-relative, `/`) and `span.start`
//!     (`line`, `column`, 1-based).
//!   - `summary`: `Success`, `CaughtMutant`, `MissedMutant`, `Unviable`, `Failure` or `Timeout`.
//!   - `log_path`: its log, relative to `mutants.out`.

use super::Fix;
use crate::violation::Violation;
use serde::Deserialize;
use std::fmt;

/// The part of `outcomes.json` xtask reads.
#[derive(Debug, Deserialize)]
pub(super) struct Lab {
    outcomes: Vec<Outcome>,
    end_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Outcome {
    scenario: Scenario,
    summary: String,
    log_path: String,
}

#[derive(Debug, Deserialize)]
enum Scenario {
    Baseline,
    Mutant(Mutant),
}

#[derive(Debug, Deserialize)]
struct Mutant {
    name: String,
    file: String,
    span: Span,
}

#[derive(Debug, Deserialize)]
struct Span {
    start: LineColumn,
}

#[derive(Debug, Deserialize)]
struct LineColumn {
    line: u32,
    column: u32,
}

impl Mutant {
    /// The change, without the `file:line:column: ` that starts `name`.
    fn mutation(&self) -> &str {
        let at = format!(
            "{}:{}:{}: ",
            self.file, self.span.start.line, self.span.start.column
        );
        self.name.strip_prefix(&at).unwrap_or(&self.name)
    }

    /// `file:line`.
    fn place(&self) -> String {
        format!("{}:{}", self.file, self.span.start.line)
    }
}

/// Parses the text of `outcomes.json`.
pub(super) fn parse(text: &str) -> Result<Lab, serde_json::Error> {
    serde_json::from_str(text)
}

/// How many mutants ended which way.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Counts {
    pub tested: usize,
    pub missed: usize,
    pub caught: usize,
    pub timeout: usize,
    pub unviable: usize,
    /// Any other outcome, each also a failure.
    pub other: usize,
}

impl Counts {
    /// Adds `other`'s counts to these, field by field: two passes' counts as one run's.
    pub(super) fn add(&mut self, other: &Counts) {
        self.tested += other.tested;
        self.missed += other.missed;
        self.caught += other.caught;
        self.timeout += other.timeout;
        self.unviable += other.unviable;
        self.other += other.other;
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.tested == 1 {
            "mutant"
        } else {
            "mutants"
        };
        write!(
            f,
            "{} {noun} tested: {} missed, {} caught, {} timed out, {} unviable",
            self.tested, self.missed, self.caught, self.timeout, self.unviable
        )?;
        if self.other > 0 {
            write!(f, ", {} other", self.other)?;
        }
        Ok(())
    }
}

/// What a run found.
#[derive(Debug, Default)]
pub(super) struct Verdict {
    /// One per missed mutant, at its file and line.
    pub missed: Vec<Violation>,
    /// Failures with no line of their own: a failed baseline, an outcome xtask does not expect,
    /// a run that did not finish.
    pub failures: Vec<(String, Fix)>,
    /// What passes but is worth knowing, each with its fix line.
    pub notes: Vec<String>,
    pub counts: Counts,
}

impl Verdict {
    /// Whether no mutant was missed and nothing failed.
    pub(super) fn passed(&self) -> bool {
        self.missed.is_empty() && self.failures.is_empty()
    }
}

/// Judges `lab`, from a run of `cmd` whose output folder is `output_dir`. A missed mutant fails,
/// and so do a failed baseline, an outcome other than these four, and a run that did not finish.
/// A mutant that timed out counts as caught: the tests did not pass on it. An unviable one did
/// not compile, so it says nothing about the tests.
pub(super) fn judge(lab: &Lab, cmd: &str, output_dir: &str) -> Verdict {
    let mut verdict = Verdict::default();
    for outcome in &lab.outcomes {
        let summary = outcome.summary.as_str();
        let mutant = match &outcome.scenario {
            Scenario::Baseline if summary == "Success" => continue,
            Scenario::Baseline => {
                let message = format!(
                    "the unmutated tree failed in cargo-mutants' copy ({summary}), so no mutant \
                     was tested; its log is {output_dir}/{}",
                    outcome.log_path
                );
                verdict.failures.push((message, Fix::Baseline));
                continue;
            }
            Scenario::Mutant(mutant) => mutant,
        };
        verdict.counts.tested += 1;
        match summary {
            "MissedMutant" => {
                verdict.counts.missed += 1;
                verdict.missed.push(Violation {
                    path: mutant.file.clone(),
                    line: mutant.span.start.line,
                    message: format!("missed mutant: no test fails on `{}`", mutant.mutation()),
                    fix: Fix::Missed.into(),
                });
            }
            "CaughtMutant" => verdict.counts.caught += 1,
            "Timeout" => {
                verdict.counts.timeout += 1;
                verdict.notes.push(format!(
                    "{}: `{}` made the tests run past cargo-mutants' time limit, so it counts as \
                     caught\n    fix: {}",
                    mutant.place(),
                    mutant.mutation(),
                    Fix::Slow
                ));
            }
            "Unviable" => verdict.counts.unviable += 1,
            other => {
                verdict.counts.other += 1;
                let message = format!(
                    "{}: cargo-mutants gave `{}` the outcome {other}, which xtask does not expect",
                    mutant.place(),
                    mutant.mutation()
                );
                let log = format!("{output_dir}/{}", outcome.log_path);
                verdict.failures.push((message, Fix::Unexpected { log }));
            }
        }
    }
    if verdict.counts.unviable > 0 {
        verdict.notes.push(format!(
            "{} of the mutants did not compile (unviable), so they say nothing about the tests; \
             {output_dir}/unviable.txt lists them\n    fix: {}",
            verdict.counts.unviable,
            Fix::Unviable
        ));
    }
    if lab.end_time.is_none() {
        let message =
            format!("`{cmd}` stopped before its run finished: outcomes.json has no end_time");
        let fix = Fix::ByHand {
            cmd: cmd.to_owned(),
        };
        verdict.failures.push((message, fix));
    }
    verdict
}
