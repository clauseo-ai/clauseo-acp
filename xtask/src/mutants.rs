//! `cargo xtask mutants`: mutation testing of the lines a branch changed.
//!
//! It writes `git diff` against the merge-base with origin/main (the base `--quick` uses) to
//! `target/xtask/mutants.diff`, then runs cargo-mutants on the mutants in those lines, with the
//! settings in `.cargo/mutants.toml`, and reads what it found from its `outcomes.json`
//! (`outcomes.rs`). Untracked files are not in the diff, so `git add` a new file first. Each
//! missed mutant (one no test fails on) is a failure at its file and line. The run is capped at
//! `MUTANTS_TIMEOUT`, whatever the machine.

mod fix;
mod outcomes;

pub(crate) use fix::Fix;

use crate::changes::{self, GitError};
use crate::output::Printer;
use crate::process::{self, Captured, Cmd, Program};
use crate::setup::CARGO_MUTANTS;
use crate::write;
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// The diff cargo-mutants reads, relative to the repo root.
const DIFF: &str = write::MUTANTS_DIFF;
/// The folder cargo-mutants makes its `mutants.out` in (`--output`), instead of the repo root.
const OUTPUT_PARENT: &str = "target/xtask";
/// Its output folder, which each run removes first.
const OUTPUT_DIR: &str = write::MUTANTS_OUTPUT;
/// What xtask reads of it.
const OUTCOMES: &str = "target/xtask/mutants.out/outcomes.json";

/// The cap on one run: the review loop waits for it, so a change whose mutants take longer is
/// split instead. The same on every machine, so a change that fits here fits in CI.
const MUTANTS_TIMEOUT: Duration = Duration::from_mins(20);

/// How long `cargo mutants --version` may run: it prints one line, so a minute means it hangs.
const VERSION_TIMEOUT: Duration = Duration::from_mins(1);

/// cargo-mutants' `--jobs`: how many mutants build and test at once, each in its own copy of the
/// tree. Each build and test run is itself parallel, so this is far below the core count. On the
/// 12-core, 24 GB Mac mini, the same 34 mutants took 162 s at 2 jobs, 129 s at 4 and 126 s at 8:
/// past 4 the cores are full, and each job only adds a build dir and memory.
const JOBS: &str = "4";

/// Runs a command in the repo root, passing each chunk of its merged output to the callback as it
/// arrives. `process::capture` in a real run; a fake in the tests.
pub(crate) type Exec<'a> = dyn FnMut(&Cmd, &mut dyn FnMut(&[u8])) -> io::Result<Captured> + 'a;

/// Runs `cargo xtask mutants` in `root`, the checkout. Exits 0 only when the run finished and no
/// mutant was missed.
pub(crate) fn run(root: &Path) -> ExitCode {
    let mut out = Printer::stdout();
    let diff = changes::diff_since_merge_base(root);
    let mut exec =
        |cmd: &Cmd, on_output: &mut dyn FnMut(&[u8])| process::capture(cmd, root, on_output);
    if run_with(&mut out, root, &std::env::temp_dir(), diff, &mut exec) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The command that runs cargo-mutants.
fn mutants_cmd() -> Cmd {
    Cmd::new(
        Program::Cargo,
        [
            "mutants",
            // Mutate every package the diff touches; from a virtual workspace's root
            // cargo-mutants would otherwise pick none.
            "--workspace",
            "--in-diff",
            DIFF,
            "--output",
            OUTPUT_PARENT,
            "--jobs",
            JOBS,
        ],
        MUTANTS_TIMEOUT,
    )
}

/// `run` in `root`, on `diff` (the branch's diff, or why git could not give it), with every
/// command going through `exec`. `temp` is the temp dir cargo-mutants makes its build dirs in.
/// Returns whether it passed.
pub(crate) fn run_with<W: Write>(
    out: &mut Printer<W>,
    root: &Path,
    temp: &Path,
    diff: Result<String, GitError>,
    exec: &mut Exec<'_>,
) -> bool {
    let cmd = mutants_cmd();
    out.header("mutants", &cmd);
    let diff = match diff {
        Ok(diff) => diff,
        Err(error) => {
            fail(
                out,
                &format!("could not diff against the merge-base with origin/main: {error}"),
                &Fix::git(&error),
            );
            return false;
        }
    };
    if diff.trim().is_empty() {
        out.note("mutants ok: nothing changed since the merge-base with origin/main");
        return true;
    }
    if !installed(out, exec) {
        return false;
    }
    if let Err(error) = write::save_mutants_diff(root, &diff) {
        fail(
            out,
            &format!("could not write {DIFF}: {error}"),
            &Fix::Write { path: DIFF },
        );
        return false;
    }
    if let Err(error) = write::remove_mutants_output(root) {
        let message = format!("could not remove {OUTPUT_DIR}, the last run's output: {error}");
        fail(out, &message, &Fix::Write { path: OUTPUT_DIR });
        return false;
    }
    let before = build_dirs(temp, root);
    let ran = exec(&cmd, &mut |chunk| out.tool_output(chunk));
    match ran {
        Ok(captured) => judge(out, root, &cmd, &captured),
        Err(error) if process::is_timeout(&error) => {
            let message = format!(
                "`{cmd}` did not finish within {}, so it was stopped",
                process::limit_text(cmd.timeout)
            );
            fail(out, &message, &Fix::Split);
            stopped_at_cap(out, root, &cmd);
            remove_left_build_dirs(out, temp, root, &before);
            false
        }
        Err(error) => {
            let message = format!("could not run `{cmd}`: {error}");
            fail(
                out,
                &message,
                &Fix::ByHand {
                    cmd: cmd.to_string(),
                },
            );
            false
        }
    }
}

/// Whether `cargo mutants --version` prints the pinned version. Prints why not.
fn installed<W: Write>(out: &mut Printer<W>, exec: &mut Exec<'_>) -> bool {
    let cmd = Cmd::new(Program::Cargo, ["mutants", "--version"], VERSION_TIMEOUT);
    let pinned = CARGO_MUTANTS.version;
    let (said, fix) = match exec(&cmd, &mut |_| {}) {
        Ok(found) if found.success => {
            let printed = found.output.trim();
            let version = printed.strip_prefix("cargo-mutants ");
            if version == Some(pinned) {
                return true;
            }
            (format!("printed `{printed}`"), Fix::Setup { pinned })
        }
        Ok(found) => {
            let first = found.output.trim().lines().next().unwrap_or_default();
            (format!("failed: {first}"), Fix::Setup { pinned })
        }
        Err(error) => (
            format!("did not run: {error}"),
            Fix::ByHand {
                cmd: cmd.to_string(),
            },
        ),
    };
    let message = format!("cargo-mutants {pinned} is not installed: `{cmd}` {said}");
    fail(out, &message, &fix);
    false
}

/// Judges a run of cargo-mutants that exited, from its `outcomes.json`.
fn judge<W: Write>(out: &mut Printer<W>, root: &Path, cmd: &Cmd, captured: &Captured) -> bool {
    let text = match std::fs::read_to_string(root.join(OUTCOMES)) {
        Ok(text) => text,
        // cargo-mutants writes no output folder when the diff holds no mutant: no Rust file
        // changed, or the changed lines hold none. The last run's folder was removed above.
        Err(error) if error.kind() == io::ErrorKind::NotFound && captured.success => {
            out.note("mutants ok: cargo-mutants found no mutant in the changed lines");
            return true;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            out.captured_output(&captured.output);
            let message = format!("`{cmd}` failed before it tested any mutant");
            fail(
                out,
                &message,
                &Fix::ByHand {
                    cmd: cmd.to_string(),
                },
            );
            return false;
        }
        Err(error) => {
            fail(
                out,
                &format!("could not read {OUTCOMES}: {error}"),
                &Fix::Unparsed,
            );
            return false;
        }
    };
    let lab = match outcomes::parse(&text) {
        Ok(lab) => lab,
        Err(error) => {
            let message = format!("could not read {OUTCOMES}: {error}");
            fail(out, &message, &Fix::Unparsed);
            return false;
        }
    };
    let verdict = outcomes::judge(&lab, &cmd.to_string(), OUTPUT_DIR);
    for missed in &verdict.missed {
        out.violation(missed);
    }
    for (message, fix) in &verdict.failures {
        fail(out, message, fix);
    }
    for note in &verdict.notes {
        out.note(format!("note: {note}"));
    }
    let mut passed = verdict.passed();
    // cargo-mutants exits 3 when a mutant timed out, which xtask counts as caught; any other
    // failure with nothing in outcomes.json to explain it is still a failure.
    if passed && !captured.success && verdict.counts.timeout == 0 {
        out.captured_output(&captured.output);
        let message = format!("`{cmd}` failed, though {OUTCOMES} shows no missed mutant");
        fail(
            out,
            &message,
            &Fix::ByHand {
                cmd: cmd.to_string(),
            },
        );
        passed = false;
    }
    let status = if passed { "ok" } else { "FAILED" };
    out.note(format!("mutants {status}: {}", verdict.counts));
    passed
}

/// After the cap: the missed mutants the stopped run had found, from its `outcomes.json` so far,
/// so the change can be split around them. Prints nothing more when the run had not written one
/// yet, or was stopped in the middle of writing it.
fn stopped_at_cap<W: Write>(out: &mut Printer<W>, root: &Path, cmd: &Cmd) {
    let Ok(text) = std::fs::read_to_string(root.join(OUTCOMES)) else {
        return;
    };
    let Ok(lab) = outcomes::parse(&text) else {
        return;
    };
    let verdict = outcomes::judge(&lab, &cmd.to_string(), OUTPUT_DIR);
    for missed in &verdict.missed {
        out.violation(missed);
    }
    out.note(format!(
        "mutants FAILED at the cap, after {}",
        verdict.counts
    ));
}

/// The start of the name of each build dir cargo-mutants makes for the tree at `root`:
/// `cargo-mutants-` and the name of its folder (cargo-mutants 27.1.0, `BuildDir::copy_from` in
/// `src/build_dir.rs`). tempfile then adds a random part and `.tmp` (`src/copy_tree.rs`).
fn build_dir_prefix(root: &Path) -> String {
    let name = root
        .file_name()
        .map_or_else(|| "unnamed".into(), |name| name.to_string_lossy());
    format!("cargo-mutants-{name}-")
}

/// The build dirs of cargo-mutants runs of the tree at `root` now in `temp`.
fn build_dirs(temp: &Path, root: &Path) -> BTreeSet<PathBuf> {
    let prefix = build_dir_prefix(root);
    let Ok(entries) = std::fs::read_dir(temp) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".tmp") && entry.path().is_dir()
        })
        .map(|entry| entry.path())
        .collect()
}

/// After the cap: removes the build dirs that appeared in `temp` during the run (those not in
/// `before`). cargo-mutants deletes them itself when it stops, but the door's SIGKILL, after its
/// grace period, can land before it has: one 20-minute run left four, of 0.5 to 0.7 GB each.
fn remove_left_build_dirs<W: Write>(
    out: &mut Printer<W>,
    temp: &Path,
    root: &Path,
    before: &BTreeSet<PathBuf>,
) {
    let left: Vec<PathBuf> = build_dirs(temp, root).difference(before).cloned().collect();
    let mut removed = 0;
    for dir in &left {
        match write::remove_mutants_build_dir(dir) {
            Ok(()) => removed += 1,
            Err(error) => {
                let message = format!(
                    "could not remove {}, a build dir cargo-mutants left when it was stopped:                      {error}",
                    dir.display()
                );
                fail(out, &message, &Fix::RemoveByHand);
            }
        }
    }
    if removed > 0 {
        let noun = if removed == 1 { "dir" } else { "dirs" };
        out.note(format!(
            "removed {removed} build {noun} cargo-mutants left in {} when it was stopped",
            temp.display()
        ));
    }
}

/// Prints a failure with no file and line.
fn fail<W: Write>(out: &mut Printer<W>, message: &str, fix: &Fix) {
    out.failure(message, &fix.to_string());
}

#[cfg(test)]
mod tests;
