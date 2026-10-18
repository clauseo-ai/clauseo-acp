//! Timing one probe on the marker file: the edits, the cargo runs, and putting the file back.
//!
//! Before the first edit, the original text is copied to `target/xtask/buildtime/<repo-relative
//! path>` under the repo root. A guard puts the original back and deletes the copy when the probe
//! ends, whether cargo succeeded or not. A guard dropped without that (a panic) restores on the
//! spot. A run that is killed leaves the copy behind, and the next `buildtime rust` run puts it back
//! before it measures anything (`restore_leftovers`).

use super::marker::{self, Marker, bump_marker};
use super::profile::warm_up_incremental;
use super::verdict::{Failure, Outcome, budget_limit, stats};
use super::{INCREMENTAL_FIX, Probe};
use crate::budgets::Budget;
use crate::clock::timed;
use crate::process::{Captured, Cmd, Program};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Where the copies of the original files wait, under the repo root.
const BACKUP_DIR: &str = "target/xtask/buildtime";

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
/// is put back before this returns, on success and on failure. `runner` runs a command and returns
/// its captured output; production passes `process::capture`.
pub(crate) fn measure(
    root: &Path,
    marker: &Marker,
    probe: Probe,
    runs: u32,
    ci: bool,
    runner: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Result<Samples, Failure> {
    let guard = Guard::save(root, marker)?;
    let edited = edit_and_run(&guard, marker, probe, runs, ci, runner);
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
    guard: &Guard,
    marker: &Marker,
    probe: Probe,
    runs: u32,
    ci: bool,
    runner: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Result<Samples, Failure> {
    let mut current = guard.original.clone();
    let mut secs = Vec::new();
    let mut warning = None;
    for run in 0..=runs {
        current = bump_marker(&current).ok_or_else(|| {
            Failure::new(
                format!("{} lost its marker line", marker.path),
                "restore the BUILDTIME_MARKER line by hand",
            )
        })?;
        fs::write(&guard.path, &current).map_err(|error| {
            Failure::new(
                format!("could not write {}: {error}", marker.path),
                "make sure the file is writable",
            )
        })?;
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

/// A marker file's original text, and the copy of it that waits under `target/xtask/buildtime`.
/// `restore` puts the original back and deletes the copy. A guard dropped before that puts it back
/// too. A failed write keeps the copy, and the failure's fix names it.
struct Guard {
    path: PathBuf,
    rel: String,
    original: String,
    base: PathBuf,
    backup: PathBuf,
    pending: bool,
}

impl Guard {
    /// Copies the original of `marker` to its backup path. Nothing is edited when the copy fails.
    fn save(root: &Path, marker: &Marker) -> Result<Guard, Failure> {
        let base = root.join(BACKUP_DIR);
        let backup = base.join(&marker.path);
        let copied = match backup.parent() {
            Some(dir) => fs::create_dir_all(dir),
            None => Ok(()),
        }
        .and_then(|()| fs::write(&backup, marker.text.as_bytes()));
        match copied {
            Ok(()) => Ok(Guard {
                path: root.join(&marker.path),
                rel: marker.path.clone(),
                original: marker.text.clone(),
                base,
                backup,
                pending: true,
            }),
            Err(error) => Err(Failure::new(
                format!(
                    "could not back up {} before editing it: {error}",
                    marker.path
                ),
                format!("make sure {BACKUP_DIR} is writable, then run again"),
            )),
        }
    }

    /// The original text back in the file, then the copy deleted.
    fn put_back(&self) -> Result<(), Failure> {
        fs::write(&self.path, self.original.as_bytes()).map_err(|error| {
            Failure::new(
                format!("could not restore {}: {error}", self.rel),
                format!(
                    "copy {} over {}; it holds the original text",
                    self.backup.display(),
                    self.rel
                ),
            )
        })?;
        fs::remove_file(&self.backup).map_err(|error| {
            Failure::new(
                format!(
                    "restored {} but could not delete its copy {}: {error}",
                    self.rel,
                    self.backup.display()
                ),
                format!(
                    "delete {}; the file is already restored",
                    self.backup.display()
                ),
            )
        })?;
        prune(&self.base, &self.backup);
        Ok(())
    }

    /// Puts the original back and deletes the copy.
    fn restore(mut self) -> Result<(), Failure> {
        self.pending = false;
        self.put_back()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // A panic or an early return lands here. A failed write keeps the copy for the next run,
        // and there is no way to print from a drop, so the outcome is not kept.
        if self.pending {
            let _restored = self.put_back().is_ok();
        }
    }
}

/// Removes the directories between `file` and `base` that are now empty.
fn prune(base: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(current) = dir.filter(|d| d.starts_with(base) && *d != base) {
        if fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// Puts back every file that an interrupted run left in `target/xtask/buildtime`: the file gets the
/// copy's bytes, and the copy is deleted. Returns one line per file, `restored <path> from an
/// interrupted buildtime run`. The first file that cannot be put back stops the walk.
pub(crate) fn restore_leftovers(root: &Path) -> Result<Vec<String>, Failure> {
    let base = root.join(BACKUP_DIR);
    let mut backups = Vec::new();
    if base.is_dir() {
        collect_files(&base, &mut backups).map_err(|error| {
            Failure::new(
                format!("could not read {BACKUP_DIR}: {error}"),
                format!("look in {BACKUP_DIR} by hand, then run again"),
            )
        })?;
    }
    backups.sort();
    let mut lines = Vec::new();
    for backup in backups {
        let Ok(relative) = backup.strip_prefix(&base) else {
            continue;
        };
        let rel = repo_path(relative);
        let restored = fs::read(&backup)
            .and_then(|bytes| fs::write(root.join(&rel), bytes))
            .and_then(|()| fs::remove_file(&backup));
        if let Err(error) = restored {
            return Err(Failure::new(
                format!("could not restore {rel} from its copy: {error}"),
                format!("copy {} over {rel} by hand", backup.display()),
            ));
        }
        prune(&base, &backup);
        lines.push(format!("restored {rel} from an interrupted buildtime run"));
    }
    Ok(lines)
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
