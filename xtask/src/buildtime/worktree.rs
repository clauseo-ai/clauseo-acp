//! `cargo xtask buildtime worktree [--runs N] [--json]`: times what a new agent worktree costs, end
//! to end, through its first `cargo xtask`, against the budget row `fresh_agent_worktree`. Its gate
//! is `alert`, so over budget is only reported. It is not part of `all`.
//!
//! Before it measures, under the checkout's lock (`lock.rs`) for the whole run: it puts back what
//! an interrupted Rust half left (`probe::restore_leftovers`); refuses a checkout whose tracked
//! files have uncommitted changes (`git status --porcelain --untracked-files=no`), since the
//! worktree is made from HEAD and the warm build is not; stops when something is already at the
//! worktree's path, `<root's folder name>-buildtime-worktree` next to the repo root, or when
//! `git worktree list` names that path though its folder is gone (xtask never deletes what it did
//! not make); and warms up with `cargo build -p clauseo-desktop -v`, not timed. The path is
//! checked again, untimed, right before each run's add: a failure there is `worktree_add`'s, and
//! no later run starts.
//!
//! Each run times six steps, each a reported-only probe without a budget row:
//! - `worktree_add`: `git worktree add --detach <path> HEAD`.
//! - `worktree_copy`: `target/debug`, `target/xtask-run`, and `target/CACHEDIR.TAG` when the
//!   checkout has one, into `<path>/target/` with their times and symlinks (`write::copy_into`,
//!   which clones each file on APFS).
//! - `worktree_touch`: every file `git ls-files` lists in the worktree gets 2000-01-01T00:00:00Z as
//!   its access and modification time (symlinks their own), older than any build output, so cargo
//!   reuses what was copied.
//! - `worktree_pnpm_install`: `pnpm -C <path>/ui install --frozen-lockfile --prefer-offline`.
//! - `worktree_first_build`: `cargo build -p clauseo-desktop --message-format=json -v` in the
//!   worktree. It fails when a `compiler-artifact` message with `"fresh": false` names another
//!   package: only clauseo-desktop may rebuild (tauri-build prints `rerun-if-changed` with absolute
//!   paths, so its build script reruns once in a new checkout). Its rustc line for clauseo-desktop,
//!   when there is one, is judged for incremental as a Rust probe's warm-up is: in CI, without
//!   incremental, the step fails and no later run starts, since each would fail the same way. A
//!   first build that passes prints `run <n>, first build: rebuilt <units>; fresh: <count>` on
//!   stderr, from those same messages.
//! - `worktree_first_xtask`: `cargo xtask --help` in the worktree, the cheapest command through
//!   the alias (clap prints the help and exits 0: no file read, no child process). It fails when it
//!   exits non-zero, or when xtask was rebuilt: the alias passes `--quiet`, so a rebuild shows as a
//!   new modification time on `target/xtask-run/debug/xtask` (cargo links the binary again on every
//!   run, so its file identity changes even when nothing is rebuilt).
//!
//! The gated probe, `fresh_worktree`, is each run's six steps added up; a run with a failed step
//! gives it no sample. After every run, a failed one too, the worktree is removed with
//! `git worktree remove --force` when the run's `git worktree add` succeeded, and must be gone
//! from disk and from `git worktree list`; when it is not, the run fails with a fix line and no
//! further run starts. When the add failed, xtask removes nothing: it names what is left at the
//! path, since it cannot tell whether git or someone else made it.
//!
//! `--json` prints one object: `{"rustc", "profile", "incremental", "ci", "runs", "passed",
//! "probes": [...]}`, each probe in the shape `report.rs` gives both halves, the six steps then
//! `fresh_worktree`, plus `"errors": [...]` for a failure that is no probe's (a worktree that could
//! not be removed), left out when there is none. A run that stopped before it measured prints
//! `{"passed": false, "errors": [...]}`. The exit code is 1 when the run fails.

mod artifacts;
mod steps;
#[cfg(test)]
mod tests;

use super::probe;
use super::profile::Incremental;
use super::report::{self, ProbeJson, probe_json};
use super::verdict::{Failure, Limit, Outcome, ProbeResult, budget_limit, passes};
use super::{Out, load_budgets, profile_line, report_over_budget, stop, toolchain};
use crate::budgets::Unit;
use crate::check;
use crate::clock::timed;
use crate::process::QuietRunner;
use serde::Serialize;
use std::path::{Path, PathBuf};
use steps::StepFailed;
#[cfg(test)]
use {
    artifacts::{built_line, package_name, rebuilt_besides},
    steps::{REBUILT_FIX, XTASK_REBUILT_FIX, same_path, touched_time, with_output, xtask_binary},
};

/// The package whose first build is timed, and the only one that build may rebuild.
pub(crate) const APP: &str = "clauseo-desktop";

/// The budget row the gated probe is compared with.
const BUDGET_ID: &str = "fresh_agent_worktree";

/// The gated probe: the six steps of a run, added up.
pub(crate) const TOTAL: &str = "fresh_worktree";

/// What is added to the root's folder name to name the worktree's folder, next to the root.
pub(crate) const SUFFIX: &str = "-buildtime-worktree";

/// What decides the incremental setting of what is timed, for the profile line.
const DECIDER: &str = "the first build's rustc line for clauseo-desktop";

/// The fix of a gated probe over its budget.
const OVER_FIX: &str = "find the slow step in the table above; a limit in budgets.toml changes \
    only with a reviewed measurement";

/// One step of a run, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Add,
    Copy,
    Touch,
    PnpmInstall,
    FirstBuild,
    FirstXtask,
}

/// The steps, in the order they run and print: the enum's order, so a step's place in `STEPS`
/// (and in what a run keeps per step) is `step as usize`.
pub(crate) const STEPS: [Step; 6] = [
    Step::Add,
    Step::Copy,
    Step::Touch,
    Step::PnpmInstall,
    Step::FirstBuild,
    Step::FirstXtask,
];

impl Step {
    /// The step's probe name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Step::Add => "worktree_add",
            Step::Copy => "worktree_copy",
            Step::Touch => "worktree_touch",
            Step::PnpmInstall => "worktree_pnpm_install",
            Step::FirstBuild => "worktree_first_build",
            Step::FirstXtask => "worktree_first_xtask",
        }
    }
}

/// The checkout, the worktree it makes, and whether CI is set.
pub(crate) struct Worktree<'a> {
    pub root: &'a Path,
    pub path: PathBuf,
    pub ci: bool,
}

/// The runs, once measured: a result per step and the gated one, the failures that are no
/// probe's (a worktree that could not be removed), and a line per first build that passed naming
/// what cargo rebuilt (`run <n>, first build: rebuilt ...`).
#[derive(Debug)]
pub(crate) struct Measured {
    pub results: Vec<ProbeResult>,
    pub errors: Vec<Failure>,
    pub notes: Vec<String>,
}

impl Measured {
    /// Whether the runs pass: no failure outside the probes, and no probe that fails them.
    pub(crate) fn passed(&self, ci: bool) -> bool {
        self.errors.is_empty() && passes(&self.results, ci)
    }
}

/// Runs `cargo xtask buildtime worktree` under the lock the caller holds: reads what it needs,
/// measures, and prints the table (or the JSON object) and the failures. `runner` runs git, cargo
/// and pnpm (production passes `process::run_quiet`). Returns whether the run passed; `Err`
/// holds one line per reason it stopped before it measured, printed already.
pub(crate) fn run(
    root: &Path,
    runs: u32,
    json: bool,
    out: &mut Out,
    diag: &mut Out,
    runner: &mut QuietRunner<'_>,
) -> Result<bool, Vec<String>> {
    for line in probe::restore_leftovers(root).map_err(|f| stop(diag, &f.message, &f.fix))? {
        diag.note(line);
    }
    let budgets = load_budgets(root, diag)?;
    let limit =
        budget_limit(&budgets, BUDGET_ID, Unit::S).map_err(|f| stop(diag, &f.message, &f.fix))?;
    let (rustc, incremental) = toolchain(root, diag)?;
    let ci = check::is_ci(std::env::var("CI").ok().as_deref());
    let path = place(root).map_err(|f| stop(diag, &f.message, &f.fix))?;
    if !json {
        out.note(&rustc);
        out.note(profile_line(&incremental, DECIDER));
        out.note(format!("worktree: {}", path.display()));
    }
    let worktree = Worktree { root, path, ci };
    let measured =
        measure(&worktree, runs, limit, runner).map_err(|f| stop(diag, &f.message, &f.fix))?;
    if !json {
        for line in report::table_lines(&measured.results) {
            out.note(line);
        }
        for line in report::warning_lines(&measured.results) {
            out.note(line);
        }
    }
    for line in &measured.notes {
        diag.note(line);
    }
    report::print_failures(&measured.results, diag);
    for failure in &measured.errors {
        diag.failure(&failure.message, &failure.fix);
    }
    report_over_budget(&measured.results, ci, OVER_FIX, diag);
    let passed = measured.passed(ci);
    if json {
        out.note(json_object(
            &rustc,
            &incremental,
            ci,
            runs,
            passed,
            &measured,
        ));
    }
    Ok(passed)
}

/// Where the worktree goes: next to `root`, named `<root's folder name>-buildtime-worktree`, in
/// `root`'s parent as `dunce::canonicalize` gives it, the form `git worktree list` prints.
pub(crate) fn place(root: &Path) -> Result<PathBuf, Failure> {
    let cannot = |why: String, fix: String| {
        Failure::new(
            format!(
                "cannot place the worktree next to {}: {why}",
                root.display()
            ),
            fix,
        )
    };
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return Err(cannot(
            "it has no parent folder".to_owned(),
            "run `cargo xtask buildtime worktree` from a checkout inside a folder: the worktree \
             goes next to the checkout"
                .to_owned(),
        ));
    };
    let parent = dunce::canonicalize(parent).map_err(|error| {
        cannot(
            format!("{}: {error}", parent.display()),
            "fix what the error names in that folder, where the worktree goes, then run again"
                .to_owned(),
        )
    })?;
    Ok(parent.join(format!("{}{SUFFIX}", name.to_string_lossy())))
}

/// Checks the checkout and that no worktree is left at the worktree's path, warms up, then
/// makes, times and removes the worktree `runs` times. `runner` runs each command in the folder
/// it is given (production passes `process::run_quiet`). `Err` is a reason not to start: nothing
/// was made then.
pub(crate) fn measure(
    worktree: &Worktree<'_>,
    runs: u32,
    limit: Limit,
    runner: &mut QuietRunner<'_>,
) -> Result<Measured, Failure> {
    worktree.refuse_uncommitted(runner)?;
    worktree.refuse_leftover(runner)?;
    worktree.warm_up(runner)?;
    let mut steps: Vec<(Vec<f64>, Option<Failure>)> = vec![(Vec::new(), None); STEPS.len()];
    let mut totals = Vec::new();
    let mut warning = None;
    let mut errors = Vec::new();
    let mut notes = Vec::new();
    for run in 1..=runs {
        let mut secs = Vec::new();
        let mut last = false;
        // Checked again right before each add, untimed: something may have come to the path since
        // the last check, during the warm-up (which can take long) or after the last removal.
        // Every later run would fail the same way, so none starts.
        if let Err(failure) = worktree.refuse_leftover(runner) {
            steps[Step::Add as usize].1.get_or_insert(failure);
            break;
        }
        for (step, (samples, failed)) in STEPS.into_iter().zip(&mut steps) {
            let (done, took) = timed(|| worktree.step(step, runner));
            match done {
                Ok(done) => {
                    samples.push(took);
                    secs.push(took);
                    warning = warning.or(done.warning);
                    notes.extend(done.built.map(|built| format!("run {run}, {built}")));
                }
                Err(StepFailed { failure, every_run }) => {
                    failed.get_or_insert(failure);
                    last = every_run;
                    break;
                }
            }
        }
        if secs.len() == STEPS.len() {
            totals.push(secs.iter().sum());
        }
        // `Step::Add` runs first, so a run that timed any step made its worktree.
        if let Err(failure) = worktree.remove(!secs.is_empty(), runner) {
            errors.push(failure);
            break;
        }
        if last {
            break;
        }
    }
    let mut results: Vec<ProbeResult> = STEPS
        .into_iter()
        .zip(steps)
        .map(|(step, (samples, failed))| ProbeResult {
            name: step.name(),
            unit: Unit::S,
            reported_only: true,
            outcome: match failed {
                Some(failure) => Outcome::Failed(failure),
                None if samples.is_empty() => Outcome::Skipped(NOT_REACHED.into()),
                None => Outcome::of_samples(step.name(), samples, None),
            },
        })
        .collect();
    let outcome = if totals.is_empty() {
        Outcome::Skipped(NO_TOTAL.into())
    } else {
        let mut outcome = Outcome::of_samples(TOTAL, totals, Some(limit));
        if let Outcome::Measured { warning: kept, .. } = &mut outcome {
            *kept = warning;
        }
        outcome
    };
    results.push(ProbeResult {
        name: TOTAL,
        unit: Unit::S,
        reported_only: false,
        outcome,
    });
    Ok(Measured {
        results,
        errors,
        notes,
    })
}

/// Why a step that never ran is skipped: the failure that stopped it fails the run.
pub(crate) const NOT_REACHED: &str = "an earlier step failed in every run";

/// Why the gated probe has no number: the failed steps fail the run.
pub(crate) const NO_TOTAL: &str = "no run finished every step";

/// The JSON object of a measured run, as the module doc says.
#[derive(Serialize)]
struct Report<'a> {
    rustc: &'a str,
    profile: &'static str,
    /// The expected setting; the first build's rustc line decides what it really used.
    incremental: bool,
    ci: bool,
    runs: u32,
    passed: bool,
    probes: Vec<ProbeJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

pub(crate) fn json_object(
    rustc: &str,
    incremental: &Incremental,
    ci: bool,
    runs: u32,
    passed: bool,
    measured: &Measured,
) -> String {
    report::json_line(&Report {
        rustc,
        profile: "dev",
        incremental: incremental.on,
        ci,
        runs,
        passed,
        probes: measured.results.iter().map(probe_json).collect(),
        errors: measured.errors.iter().map(|f| f.message.clone()).collect(),
    })
}
