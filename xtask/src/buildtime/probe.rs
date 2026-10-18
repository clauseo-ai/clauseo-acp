//! Timing one probe on the marker file: the edits, the cargo runs, and putting the file back.
//!
//! Before the first edit, the original is copied to `target/xtask/buildtime/<repo path>`, and a
//! copy is never replaced. A guard (`probe/guard.rs`) puts the original back and deletes the copy
//! when the probe ends, and on a panic, but never over an edit made during the run. A killed run
//! leaves its copy, and the next `buildtime rust` run puts it back only where the file differs by
//! the marker's number alone (`restore_leftovers`).

mod guard;

use super::marker::{self, Leftover, Marker, bump_marker, leftover};
use super::profile::warm_up_incremental;
use super::verdict::{Failure, Outcome, budget_limit, stats};
use super::{INCREMENTAL_FIX, Probe};
use crate::budgets::Budget;
use crate::clock::timed;
use crate::process::{Captured, Cmd, Program};
use guard::{BACKUP_DIR, Guard, conflict, copy_name, prune};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The timed runs of one probe, and the warning that its build was not incremental, if it was not.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Samples {
    pub secs: Vec<f64>,
    pub warning: Option<String>,
}

/// Runs `probe` for the member in `dir`, and returns its outcome. Cargo runs at `root`; `dir` only
/// holds the marker file.
pub(crate) fn run_probe(
    root: &Path,
    dir: &str,
    probe: &Probe,
    budgets: &[Budget],
    runs: u32,
    ci: bool,
    runner: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Outcome {
    let limit = match budget_limit(budgets, probe.budget_id) {
        Ok(limit) => limit,
        Err(failure) => return Outcome::Failed(failure),
    };
    let Some(marker) = marker::find(root, dir) else {
        let lib = marker::in_crate(dir, "src/lib.rs");
        return Outcome::Failed(Failure::new(
            format!("{lib}:1: no pub const BUILDTIME_MARKER: u64 in src/lib.rs or src/main.rs"),
            "add pub const BUILDTIME_MARKER: u64 = 0; to the crate root",
        ));
    };
    match measure(root, &marker, *probe, runs, ci, runner) {
        Err(failure) => Outcome::Failed(failure),
        Ok(samples) => match stats(&samples.secs) {
            Some(stats) => Outcome::Measured {
                stats,
                budget_id: probe.budget_id,
                budget_s: limit.seconds,
                gate: limit.gate,
                warning: samples.warning,
            },
            None => Outcome::Failed(Failure::new(
                format!("no timed runs for {}", probe.name),
                "run `cargo xtask buildtime` with --runs 1 or more",
            )),
        },
    }
}

/// Runs the warm-up (`cargo <subcommand> -v`) and then `runs` timed runs of `cargo <subcommand>`.
/// Each run writes the marker's next value first, so cargo rebuilds the crate each time. The file
/// is put back before this returns, on success and on failure, unless someone edited it during the
/// run: then nothing more is written, the copy is kept, and the probe fails with a conflict. `runner` runs a command and returns
/// its captured output; production passes `process::capture`.
pub(crate) fn measure(
    root: &Path,
    marker: &Marker,
    probe: Probe,
    runs: u32,
    ci: bool,
    runner: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Result<Samples, Failure> {
    let mut guard = Guard::save(root, marker)?;
    let edited = edit_and_run(&mut guard, marker, probe, runs, ci, runner);
    let restored = guard.restore();
    match (edited, restored) {
        (Ok(samples), Ok(())) => Ok(samples),
        (Ok(_), Err(restore)) => Err(restore),
        (Err(failure), Ok(())) => Err(failure),
        (Err(mut failure), Err(restore)) => {
            failure.message = format!("{}; {}", failure.message, restore.message);
            failure.fix = format!("{}; {}", failure.fix, restore.fix);
            Err(failure)
        }
    }
}

/// The edit-and-run loop of `measure`: the warm-up, then the timed runs. The warm-up's output is
/// judged before any timed run, so a probe that cannot be trusted stops after one run.
fn edit_and_run(
    guard: &mut Guard,
    marker: &Marker,
    probe: Probe,
    runs: u32,
    ci: bool,
    runner: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Result<Samples, Failure> {
    let mut secs = Vec::new();
    let mut warning = None;
    for run in 0..=runs {
        let next = bump_marker(&guard.last).ok_or_else(|| {
            Failure::new(
                format!("{} lost its marker line", marker.path),
                "restore the BUILDTIME_MARKER line by hand",
            )
        })?;
        guard.write(next)?;
        let warm_up = run == 0;
        let cmd = if warm_up {
            Cmd::new(Program::Cargo, [probe.subcommand, "-v"])
        } else {
            Cmd::new(Program::Cargo, [probe.subcommand])
        };
        let (captured, elapsed) = timed(|| runner(&cmd));
        let captured = captured.map_err(|error| {
            Failure::new(
                format!("could not run `{cmd}`: {error}"),
                format!("run `{cmd}` to see the error"),
            )
        })?;
        if !captured.success {
            return Err(Failure {
                message: format!("{cmd} failed"),
                fix: format!("run `{cmd}` to see the error"),
                output: Some(captured.output),
            });
        }
        if warm_up {
            warning = judge_warm_up(&captured.output, probe, ci)?;
        } else {
            secs.push(elapsed);
        }
    }
    Ok(Samples { secs, warning })
}

/// Reads the warm-up's rustc lines for the probe's crate. Fails when cargo did not rebuild the
/// crate, and when CI is set and the crate builds without incremental. Otherwise returns the
/// warning that a non-incremental build is not comparable with the budget, or `None` when it is
/// incremental.
pub(crate) fn judge_warm_up(
    output: &str,
    probe: Probe,
    ci: bool,
) -> Result<Option<String>, Failure> {
    match warm_up_incremental(output, probe.package) {
        None => Err(Failure::new(
            format!(
                "`cargo {}` did not rebuild {} after its marker changed; it is not in the default members' build graph",
                probe.subcommand, probe.package
            ),
            format!(
                "add {} to the build graph of the default members (default-members in Cargo.toml)",
                probe.package
            ),
        )),
        Some(true) => Ok(None),
        Some(false) if ci => Err(Failure::new(
            format!(
                "refusing to measure: CI is set and {} builds without incremental",
                probe.package
            ),
            INCREMENTAL_FIX,
        )),
        Some(false) => Ok(Some(format!(
            "warning: {} builds without incremental; its timing is not comparable with the budget",
            probe.package
        ))),
    }
}

/// Looks at every file an interrupted run left in `target/xtask/buildtime`, before anything is
/// written. A copy goes back only where the file differs from it by the marker's number. A file
/// that changed otherwise, or is gone, is a conflict: nothing is written then, and each conflict
/// is named. A file without a marker line is not a copy and is left alone. A temporary file of an
/// interrupted save is deleted. Returns one line per restored file,
/// `restored <path> from an interrupted buildtime run`.
pub(crate) fn restore_leftovers(root: &Path) -> Result<Vec<String>, Failure> {
    let base = root.join(BACKUP_DIR);
    let mut files = Vec::new();
    if base.is_dir() {
        collect_files(&base, &mut files).map_err(|error| {
            Failure::new(
                format!("could not read {BACKUP_DIR}: {error}"),
                format!("look in {BACKUP_DIR} by hand, then run again"),
            )
        })?;
    }
    files.sort();
    let mut plans = Vec::new();
    let mut conflicts = Vec::new();
    for file in files {
        let Ok(relative) = file.strip_prefix(&base) else {
            continue;
        };
        let rel = repo_path(relative);
        if Path::new(&rel).extension().is_some_and(|ext| ext == "tmp") {
            fs::remove_file(&file).map_err(|error| {
                Failure::new(
                    format!("could not delete {}: {error}", copy_name(&rel)),
                    format!("delete {} by hand", file.display()),
                )
            })?;
            continue;
        }
        let bytes = fs::read(&file).map_err(|error| {
            Failure::new(
                format!("could not read {}: {error}", copy_name(&rel)),
                format!("look at {} by hand, then run again", file.display()),
            )
        })?;
        let Ok(copy_text) = String::from_utf8(bytes.clone()) else {
            continue;
        };
        let repo_text = fs::read_to_string(root.join(&rel)).ok();
        match leftover(repo_text.as_deref(), &copy_text) {
            Leftover::NotACopy => {}
            Leftover::Restore => plans.push((file, rel, bytes)),
            Leftover::Missing => {
                conflicts.push(conflict(
                    &rel,
                    "is missing after an interrupted buildtime run",
                ));
            }
            Leftover::Conflict => {
                conflicts.push(conflict(&rel, "changed after an interrupted buildtime run"));
            }
        }
    }
    if !conflicts.is_empty() {
        return Err(combine(&conflicts));
    }
    let mut lines = Vec::new();
    for (copy, rel, bytes) in plans {
        let restored = fs::write(root.join(&rel), &bytes).and_then(|()| fs::remove_file(&copy));
        if let Err(error) = restored {
            return Err(Failure::new(
                format!("could not restore {rel} from its copy: {error}"),
                format!("copy {} over {rel} by hand", copy.display()),
            ));
        }
        prune(&base, &copy);
        lines.push(format!("restored {rel} from an interrupted buildtime run"));
    }
    Ok(lines)
}

/// The conflicts as one failure: their messages and fixes, joined.
fn combine(conflicts: &[Failure]) -> Failure {
    let messages: Vec<String> = conflicts.iter().map(|c| c.message.clone()).collect();
    let fixes: Vec<String> = conflicts.iter().map(|c| c.fix.clone()).collect();
    Failure::new(messages.join("; "), fixes.join("; "))
}

/// Every file under `dir`, recursively.
fn collect_files(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// A relative path with `/` between its parts, as the messages print it.
fn repo_path(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests;
