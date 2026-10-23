//! `cargo xtask mutants`: mutation testing of the lines a branch changed.
//!
//! It writes `git diff` against the merge-base with origin/main (the base `--quick` uses) to
//! `target/xtask/mutants.diff`, then runs cargo-mutants on the mutants in those lines, with the
//! settings in `.cargo/mutants.toml`, and reads what it found from its `outcomes.json`
//! (`outcomes.rs`). That is one pass without features and without `crates/protocol/src/wire.rs`,
//! and, when the diff changes that file, a second pass on it alone with protocol's `specta`
//! feature (`pass.rs` says why); a run of two passes prints each pass's counts, then their sum.
//! Untracked files are not in the diff, so a run with an untracked `.rs` file fails before
//! cargo-mutants starts, naming the file to `git add`. Each missed mutant (one no test fails on)
//! is a failure at its file and line. Both passes together are capped at `MUTANTS_TIMEOUT`,
//! calibrated on one machine (see there).

mod build_dirs;
mod fix;
mod outcomes;
mod pass;

pub(crate) use fix::Fix;

use crate::changes::{self, GitError};
use crate::clock;
use crate::output::Printer;
use crate::process::{self, Cmd, Program, Ran, Status};
use crate::setup::CARGO_MUTANTS;
use crate::write;
use build_dirs::{build_dirs, remove_left_build_dirs};
use pass::{Judged, Pass};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// The folder cargo-mutants makes its `mutants.out` in (`--output`), instead of the repo root,
/// and the diff is saved in.
const OUTPUT_PARENT: &str = "target/xtask";
/// The diff's name in `OUTPUT_PARENT`.
const DIFF_NAME: &str = "mutants.diff";
/// The diff cargo-mutants reads, relative to the repo root.
const DIFF: &str = "target/xtask/mutants.diff";
/// The output folder's name in `OUTPUT_PARENT`.
const OUTPUT_NAME: &str = "mutants.out";
/// Its output folder, which each run removes first.
const OUTPUT_DIR: &str = "target/xtask/mutants.out";
/// What xtask reads of it.
const OUTCOMES: &str = "target/xtask/mutants.out/outcomes.json";

/// The cap on one run, both passes together: the review loop waits for it, so a change whose
/// mutants take longer is split instead. Calibrated on the 12-core Apple M6 Mac only, where 20
/// minutes covered about 270 mutants; a slower machine covers fewer, so a change that fits there
/// can fail on a CI runner.
/// CI's mutants job runs about half as fast; recalibrating it there is issue #40.
const MUTANTS_TIMEOUT: Duration = Duration::from_mins(20);

/// How long cargo-mutants has between the door's SIGTERM and its SIGKILL at the cap. It starts
/// each cargo and nextest child in a process group of its own (27.1.0's src/process/unix.rs:37),
/// so the door's signals never reach them; it handles SIGTERM itself (ctrlc with its
/// `termination` feature, src/interrupt.rs:16), sends SIGTERM to each child's group
/// (src/process.rs:112) and exits. With the default five seconds, one capped run left four build
/// dirs behind (see `remove_left_build_dirs`).
const MUTANTS_KILL_GRACE: Duration = Duration::from_mins(1);

/// How long `git ls-files` may run: it lists files from the index, so a minute means it hangs.
const GIT_TIMEOUT: Duration = Duration::from_mins(1);

/// How long `cargo mutants --version` may run: it prints one line, so a minute means it hangs.
const VERSION_TIMEOUT: Duration = Duration::from_mins(1);

/// cargo-mutants' `--jobs`: how many mutants build and test at once, each in its own copy of the
/// tree. Each build and test run is itself parallel, so this is far below the core count. On the
/// 12-core, 24 GB Mac mini, the same 34 mutants took 162 s at 2 jobs, 129 s at 4 and 126 s at 8:
/// past 4 the cores are full, and each job only adds a build dir and memory. Tuned on that Mac
/// only (the 12-core Apple M6); recalibrating it with `MUTANTS_TIMEOUT` on the CI runner is issue
/// #40.
const JOBS: &str = "4";

/// Runs a command in the repo root, passing each chunk of its merged output to the callback as it
/// arrives. `process::capture` in a real run; a fake in the tests.
pub(crate) type Exec<'a> = dyn FnMut(&Cmd, &mut dyn FnMut(&[u8])) -> io::Result<Ran> + 'a;

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
    let main = Pass::main();
    out.header("mutants", &main.cmd);
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
    if !all_tracked(out, exec) {
        return false;
    }
    if diff.trim().is_empty() {
        out.note("mutants ok: nothing changed since the merge-base with origin/main");
        return true;
    }
    if !installed(out, exec) {
        return false;
    }
    if let Err(error) = write::write_file_in(&root.join(OUTPUT_PARENT), DIFF_NAME, diff.as_bytes())
    {
        fail(
            out,
            &format!("could not write {DIFF}: {error}"),
            &Fix::Write { path: DIFF },
        );
        return false;
    }
    for (parent, dir) in [
        (OUTPUT_PARENT, OUTPUT_DIR),
        (pass::WIRE_OUTPUT_PARENT, pass::WIRE_OUTPUT_DIR),
    ] {
        if let Err(error) = remove_output(root, parent) {
            let message = format!("could not remove {dir}, the last run's output: {error}");
            fail(out, &message, &Fix::Write { path: dir });
            return false;
        }
    }
    let (first, spent) = clock::timed(|| run_pass(out, root, temp, &main, exec));
    // A pass stopped at the cap or never started has printed why; the cap is spent either way.
    let Some(first) = first else {
        return false;
    };
    if !pass::touches_wire(&diff) {
        if first == Judged::Failed {
            return false;
        }
        let passed = first.passed();
        out.note(format!(
            "mutants {}: {}",
            pass::status(passed),
            first.summary()
        ));
        return passed;
    }
    out.note(format!("mutants, {}: {}", main.label, first.summary()));
    // `timed` gives a finite, non-negative count of seconds, which always converts.
    let spent = Duration::try_from_secs_f64(spent).unwrap_or(MUTANTS_TIMEOUT);
    run_wire(out, root, temp, spent, exec, &first)
}

/// The wire pass, after a main pass that came out `first` in `spent`: it gets what is left of
/// the cap, and the run passes only when both passes did. Prints its counts, then both passes'.
fn run_wire<W: Write>(
    out: &mut Printer<W>,
    root: &Path,
    temp: &Path,
    spent: Duration,
    exec: &mut Exec<'_>,
    first: &Judged,
) -> bool {
    let Some(left) = pass::remaining(spent) else {
        let message = format!(
            "the first pass used the whole {} cap, so the mutants of {} were not tested",
            process::limit_text(MUTANTS_TIMEOUT),
            pass::WIRE
        );
        fail(out, &message, &Fix::Split);
        return false;
    };
    let wire = Pass::wire(left);
    out.header("mutants", &wire.cmd);
    let Some(second) = run_pass(out, root, temp, &wire, exec) else {
        return false;
    };
    out.note(format!("mutants, {}: {}", wire.label, second.summary()));
    let passed = first.passed() && second.passed();
    let mut counts = first.counts();
    counts.add(&second.counts());
    out.note(format!(
        "mutants {}: {counts}, in both passes",
        pass::status(passed)
    ));
    passed
}

/// Runs `pass` through `exec` and judges it. `None` when it ran past its limit or did not
/// start, which is printed, as are its missed mutants and failures; its summary line is not.
fn run_pass<W: Write>(
    out: &mut Printer<W>,
    root: &Path,
    temp: &Path,
    pass: &Pass,
    exec: &mut Exec<'_>,
) -> Option<Judged> {
    let cmd = &pass.cmd;
    let before = build_dirs(temp, root);
    match exec(cmd, &mut |chunk| out.tool_output(chunk)) {
        Ok(ran) if ran.status == Status::TimedOut => {
            fail(out, &process::timeout_message(cmd), &Fix::Split);
            stopped_at_cap(out, root, pass);
            remove_left_build_dirs(out, temp, root, &before);
            None
        }
        Ok(captured) => Some(judge(out, root, pass, &captured)),
        Err(error) => {
            let message = format!("could not run `{cmd}`: {error}");
            fail(
                out,
                &message,
                &Fix::ByHand {
                    cmd: cmd.to_string(),
                },
            );
            None
        }
    }
}

/// The command that lists the untracked `.rs` files git does not ignore, NUL-separated.
fn untracked_cmd() -> Cmd {
    Cmd::new(
        Program::Git,
        [
            "ls-files",
            "-z",
            "--others",
            "--exclude-standard",
            "--",
            "*.rs",
        ],
        GIT_TIMEOUT,
    )
}

/// Whether every `.rs` file git does not ignore is tracked: `--in-diff` gets only the tracked
/// files' changes, so cargo-mutants would mutate none of an untracked one and still pass. Prints
/// a failure naming each untracked file, or why git could not list them.
fn all_tracked<W: Write>(out: &mut Printer<W>, exec: &mut Exec<'_>) -> bool {
    let cmd = untracked_cmd();
    let (error, fix) = match exec(&cmd, &mut |_| {}) {
        Ok(listed) if listed.status == Status::TimedOut => (
            GitError::TimedOut(process::timeout_message(&cmd)),
            Fix::GitTimedOut,
        ),
        Ok(listed) if listed.success() => {
            let untracked: Vec<&str> = listed
                .stdout
                .split('\0')
                .filter(|f| !f.is_empty())
                .collect();
            for file in &untracked {
                let message =
                    format!("{file} is untracked, so cargo-mutants would mutate none of it");
                fail(
                    out,
                    &message,
                    &Fix::Untracked {
                        file: (*file).to_owned(),
                    },
                );
            }
            return untracked.is_empty();
        }
        Ok(listed) => {
            let error = GitError::Failed {
                command: cmd.to_string(),
                stderr: listed.stdout.trim().to_owned(),
            };
            let fix = Fix::ByHand {
                cmd: cmd.to_string(),
            };
            (error, fix)
        }
        Err(error) => (GitError::NotStarted(error.to_string()), Fix::GitNotStarted),
    };
    let message = format!("could not list the untracked .rs files: {error}");
    fail(out, &message, &fix);
    false
}

/// Removes `mutants.out` in `parent` under `root`, the folder an earlier run of cargo-mutants
/// left, so its `outcomes.json` is never read as this run's. A missing folder is not an error.
fn remove_output(root: &Path, parent: &str) -> io::Result<()> {
    match write::remove_in(&root.join(parent), OUTPUT_NAME) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Whether `cargo mutants --version` prints the pinned version. Prints why not.
fn installed<W: Write>(out: &mut Printer<W>, exec: &mut Exec<'_>) -> bool {
    let cmd = Cmd::new(Program::Cargo, ["mutants", "--version"], VERSION_TIMEOUT);
    let pinned = CARGO_MUTANTS.version;
    let not_installed = format!("cargo-mutants {pinned} is not installed: `{cmd}`");
    let (message, fix) = match exec(&cmd, &mut |_| {}) {
        Ok(found) if found.status == Status::TimedOut => (
            format!(
                "cargo-mutants {pinned} could not be checked: {}",
                process::timeout_message(&cmd)
            ),
            Fix::ByHand {
                cmd: cmd.to_string(),
            },
        ),
        Ok(found) if found.success() => {
            let printed = found.stdout.trim();
            let version = printed.strip_prefix("cargo-mutants ");
            if version == Some(pinned) {
                return true;
            }
            (
                format!("{not_installed} printed `{printed}`"),
                Fix::Setup { pinned },
            )
        }
        Ok(found) => {
            let first = found.stdout.trim().lines().next().unwrap_or_default();
            (
                format!("{not_installed} failed: {first}"),
                Fix::Setup { pinned },
            )
        }
        Err(error) => (
            format!("{not_installed} did not run: {error}"),
            Fix::ByHand {
                cmd: cmd.to_string(),
            },
        ),
    };
    fail(out, &message, &fix);
    false
}

/// Judges a pass of cargo-mutants that exited, from its `outcomes.json`. Prints its missed
/// mutants, failures and notes, but not its summary line.
fn judge<W: Write>(out: &mut Printer<W>, root: &Path, pass: &Pass, captured: &Ran) -> Judged {
    let (cmd, outcomes) = (&pass.cmd, pass.outcomes);
    let text = match std::fs::read_to_string(root.join(outcomes)) {
        Ok(text) => text,
        // cargo-mutants writes no output folder when the diff holds no mutant: no Rust file
        // changed, or the changed lines hold none. The last run's folder was removed above.
        Err(error) if error.kind() == io::ErrorKind::NotFound && captured.success() => {
            return Judged::NoMutant;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            out.captured_output(&captured.stdout);
            let message = format!("`{cmd}` failed before it tested any mutant");
            fail(
                out,
                &message,
                &Fix::ByHand {
                    cmd: cmd.to_string(),
                },
            );
            return Judged::Failed;
        }
        Err(error) => {
            fail(
                out,
                &format!("could not read {outcomes}: {error}"),
                &Fix::Unparsed,
            );
            return Judged::Failed;
        }
    };
    let lab = match outcomes::parse(&text) {
        Ok(lab) => lab,
        Err(error) => {
            let message = format!("could not read {outcomes}: {error}");
            fail(out, &message, &Fix::Unparsed);
            return Judged::Failed;
        }
    };
    let verdict = outcomes::judge(&lab, &cmd.to_string(), pass.output_dir);
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
    if passed && !captured.success() && verdict.counts.timeout == 0 {
        out.captured_output(&captured.stdout);
        let message = format!("`{cmd}` failed, though {outcomes} shows no missed mutant");
        fail(
            out,
            &message,
            &Fix::ByHand {
                cmd: cmd.to_string(),
            },
        );
        passed = false;
    }
    Judged::Counted {
        passed,
        counts: verdict.counts,
    }
}

/// After the cap: the missed mutants the stopped run had found, from its `outcomes.json` so far,
/// so the change can be split around them. Prints nothing more when the run had not written one
/// yet, or was stopped in the middle of writing it.
fn stopped_at_cap<W: Write>(out: &mut Printer<W>, root: &Path, pass: &Pass) {
    let Ok(text) = std::fs::read_to_string(root.join(pass.outcomes)) else {
        return;
    };
    let Ok(lab) = outcomes::parse(&text) else {
        return;
    };
    let verdict = outcomes::judge(&lab, &pass.cmd.to_string(), pass.output_dir);
    for missed in &verdict.missed {
        out.violation(missed);
    }
    out.note(format!(
        "mutants FAILED at the cap, after {}",
        verdict.counts
    ));
}

/// Prints a failure with no file and line.
fn fail<W: Write>(out: &mut Printer<W>, message: &str, fix: &Fix) {
    out.failure(message, &fix.to_string());
}

#[cfg(test)]
mod tests;
