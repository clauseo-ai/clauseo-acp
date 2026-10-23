//! The steps of a worktree run, the refusal and warm-up before them, and the removal after each
//! run, as the module doc of `worktree.rs` lists them.

use super::super::one_line;
use super::super::probe::judge_incremental;
use super::super::profile::warm_up_incremental;
use super::super::verdict::Failure;
use super::artifacts::{built_line, rebuilt_besides};
use super::{APP, Step, Worktree};
use crate::parse::clippy_render;
use crate::process::{self, Cmd, Program, QuietRunner, Ran, Status, TimeoutFix};
use crate::write;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// git's commands here (status, worktree add, ls-files, worktree list) each take seconds on a
/// checkout this size, so 5 min means one hangs (on a lock, say).
const GIT_TIMEOUT: Duration = Duration::from_mins(5);

/// `git worktree remove` deletes the copied build output, gigabytes on a slow disk.
const REMOVE_TIMEOUT: Duration = Duration::from_mins(10);

/// pnpm installs from its store, in seconds, but fetches what the store lacks over the network.
const PNPM_TIMEOUT: Duration = Duration::from_mins(30);

/// The warm-up can build every dependency cold on a slow machine, as a Rust probe's can.
const BUILD_TIMEOUT: Duration = Duration::from_hours(1);

/// `cargo xtask --help` builds xtask from scratch when the copy did not bring it.
const XTASK_TIMEOUT: Duration = Duration::from_mins(30);

/// 2000-01-01T00:00:00Z, 10957 days after the epoch: the time every tracked file gets, older
/// than any build output.
pub(crate) fn touched_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(10_957 * 24)
}

/// The fix when the first build rebuilt more than the app. `cargo build -v` prints a `Dirty`
/// line with the reason for each crate it rebuilds that it built before, and none for a crate it
/// builds for the first time, only its `Compiling` line (cargo 1.99); the failure shows them.
pub(crate) const REBUILT_FIX: &str = "read why cargo rebuilt each one in the `Dirty` lines \
    above (`cargo build -v` prints them); one with no `Dirty` line was built for the first \
    time, so the copied target/debug held no build of it with the worktree's settings; fix that \
    cause, then run again";

/// The fix when the first build rebuilt more than the app and printed no `Dirty` line at all.
pub(crate) const NO_DIRTY_FIX: &str = "cargo printed no `Dirty` line, so it built each one for \
    the first time: the copied target/debug held no build of them with the worktree's settings \
    (features, environment, `.cargo/config.toml`); find what differs from the repo root, then \
    run again";

/// The fix when the first `cargo xtask` rebuilt xtask. The alias passes `--quiet`, so its own
/// `-v` run shows the `Dirty` line.
pub(crate) const XTASK_REBUILT_FIX: &str = "make a worktree the same way and run \
    `cargo run --locked --package xtask --target-dir target/xtask-run -v -- --help` in it: its \
    `Dirty` line says why cargo rebuilt xtask";

/// The fix when xtask's build output is not in the checkout.
pub(crate) const XTASK_RUN_FIX: &str = "run `cargo xtask --help` from the repo root (the alias \
    builds xtask into target/xtask-run), then run again";

/// What a step that passed found: for the first build, its not-incremental warning when it has
/// one, and what cargo rebuilt (`built_line`).
#[derive(Debug, Default)]
pub(crate) struct Done {
    pub warning: Option<String>,
    pub built: Option<String>,
}

/// A step that failed, and whether every later run would fail it the same way, so none starts.
#[derive(Debug)]
pub(crate) struct StepFailed {
    pub failure: Failure,
    pub every_run: bool,
}

impl From<Failure> for StepFailed {
    fn from(failure: Failure) -> StepFailed {
        StepFailed {
            failure,
            every_run: false,
        }
    }
}

impl Worktree<'_> {
    /// Runs one step.
    pub(super) fn step(
        &self,
        step: Step,
        runner: &mut QuietRunner<'_>,
    ) -> Result<Done, StepFailed> {
        let done = match step {
            Step::Add => {
                let path = self.path.to_string_lossy();
                let add = ["worktree", "add", "--detach", &path, "HEAD"];
                let cmd = Cmd::new(Program::Git, add, GIT_TIMEOUT);
                ran(runner, &cmd, self.root, Where::Checkout).map(|_| ())
            }
            Step::Copy => self.copy(),
            Step::Touch => self.touch(runner),
            Step::PnpmInstall => {
                let ui = self.path.join("ui");
                let ui = ui.to_string_lossy();
                let install = [
                    "-C",
                    &ui,
                    "install",
                    "--frozen-lockfile",
                    "--prefer-offline",
                ];
                let cmd = Cmd::new(Program::Pnpm, install, PNPM_TIMEOUT);
                ran(runner, &cmd, &self.path, Where::Worktree).map(|_| ())
            }
            Step::FirstBuild => return self.first_build(runner),
            Step::FirstXtask => self.first_xtask(runner),
        };
        done?;
        Ok(Done::default())
    }

    /// Fails when tracked files have uncommitted changes, or when git cannot say.
    pub(super) fn refuse_uncommitted(&self, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let status = ["status", "--porcelain", "--untracked-files=no"];
        let cmd = Cmd::new(Program::Git, status, GIT_TIMEOUT);
        let listed = ran(runner, &cmd, self.root, Where::Checkout)?;
        if listed.stdout.trim().is_empty() {
            return Ok(());
        }
        Err(Failure::new(
            format!(
                "refusing to measure: tracked files have uncommitted changes (`{cmd}` lists them), \
                 and the worktree is made from HEAD, so its first build would not match the warm \
                 checkout"
            ),
            "commit them or stash them (`git stash`), then run again",
        ))
    }

    /// Fails when something is at the worktree's path, or when `git worktree list` names it
    /// though its folder is gone: xtask never deletes what this run did not make.
    pub(super) fn refuse_leftover(&self, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let path = self.path.display();
        let left = "an interrupted `cargo xtask buildtime worktree` run may have left it";
        if fs::symlink_metadata(&self.path).is_ok() {
            return Err(Failure::new(
                format!("{path} is already there; {left}"),
                format!(
                    "if `git worktree list` names it, remove it with `git worktree remove \
                     {path}`, which refuses a worktree with modified or untracked files; \
                     otherwise move it away. xtask never deletes what this run did not make"
                ),
            ));
        }
        if self.listed(runner)? {
            return Err(Failure::new(
                format!("`git worktree list` names {path}, whose folder is gone; {left}"),
                format!(
                    "run `git worktree prune`, which forgets every worktree whose folder is gone \
                     unless it is locked: first `git worktree unlock {path}` if \
                     `git worktree list` shows it locked; then run again"
                ),
            ));
        }
        Ok(())
    }

    /// `cargo build -p clauseo-desktop -v` in the checkout, not timed. Fails when it fails, and in
    /// CI when its rustc line, if it rebuilt the app, shows it built without incremental.
    pub(super) fn warm_up(&self, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let cmd = Cmd::new(Program::Cargo, ["build", "-p", APP, "-v"], BUILD_TIMEOUT);
        let built = ran(runner, &cmd, self.root, Where::Checkout)?;
        match warm_up_incremental(&built.stderr, APP) {
            Some(incremental) => judge_incremental(APP, incremental, self.ci).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Copies the build output into `<path>/target/` with `write::copy_into`: `debug` and
    /// `xtask-run`, which must be there, then `CACHEDIR.TAG` when it is (cargo writes it only
    /// when it makes `target/` itself). Nothing is copied when one that must be there is not.
    pub(super) fn copy(&self) -> Result<(), Failure> {
        let target = self.path.join("target");
        let required = [
            (
                "debug",
                format!("run `cargo build -p {APP}` from the repo root, then run again"),
            ),
            ("xtask-run", XTASK_RUN_FIX.to_owned()),
        ];
        for (name, fix) in &required {
            if fs::symlink_metadata(self.root.join("target").join(name)).is_err() {
                return Err(Failure::new(
                    format!("target/{name} is not in this checkout"),
                    fix.as_str(),
                ));
            }
        }
        let tag = self.root.join("target/CACHEDIR.TAG");
        let tag = fs::symlink_metadata(&tag).is_ok().then_some("CACHEDIR.TAG");
        for name in required.iter().map(|(name, _)| *name).chain(tag) {
            let from = self.root.join("target").join(name);
            write::copy_into(&from, &target, name).map_err(|error| {
                Failure::new(
                    format!(
                        "could not copy target/{name} into {}: {error}",
                        target.display()
                    ),
                    "fix what the error names, then run again",
                )
            })?;
        }
        Ok(())
    }

    /// Gives every file `git ls-files -z` lists in the worktree `touched_time()`.
    pub(super) fn touch(&self, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let cmd = Cmd::new(Program::Git, ["ls-files", "-z"], GIT_TIMEOUT);
        let listed = ran(runner, &cmd, &self.path, Where::Worktree)?;
        for file in listed.stdout.split('\0').filter(|file| !file.is_empty()) {
            write::set_times_in(&self.path, file, touched_time()).map_err(|error| {
                Failure::new(
                    format!("could not set the times of {file} in the worktree: {error}"),
                    "fix what the error names, then run again",
                )
            })?;
        }
        Ok(())
    }

    /// The first build in the worktree. Fails when it rebuilt another package than the app, and
    /// judges the app's rustc line for incremental: without it, in CI, the step fails in a way
    /// every run would repeat, so no later run starts (as a Rust probe's warm-up refuses).
    pub(super) fn first_build(&self, runner: &mut QuietRunner<'_>) -> Result<Done, StepFailed> {
        let build = ["build", "-p", APP, "--message-format=json", "-v"];
        let cmd = Cmd::new(Program::Cargo, build, BUILD_TIMEOUT);
        let built = ran(runner, &cmd, &self.path, Where::Worktree)?;
        let others = rebuilt_besides(&built.stdout, APP);
        if !others.is_empty() {
            let dirty: Vec<&str> = built
                .stderr
                .lines()
                .filter(|line| line.trim_start().starts_with("Dirty "))
                .collect();
            let (fix, output) = if dirty.is_empty() {
                (NO_DIRTY_FIX, None)
            } else {
                (REBUILT_FIX, Some(format!("{}\n", dirty.join("\n"))))
            };
            return Err(StepFailed::from(Failure {
                message: format!(
                    "`{cmd}` rebuilt more than {APP} in the worktree: {}",
                    others.join(", ")
                ),
                fix: fix.to_owned(),
                output,
            }));
        }
        let warning = match warm_up_incremental(&built.stderr, APP) {
            Some(incremental) => {
                judge_incremental(APP, incremental, self.ci).map_err(|failure| StepFailed {
                    failure,
                    every_run: true,
                })?
            }
            None => None,
        };
        Ok(Done {
            warning,
            built: Some(built_line(&built.stdout)),
        })
    }

    /// `cargo xtask --help` in the worktree. Fails when xtask's binary changed.
    pub(super) fn first_xtask(&self, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let binary = xtask_binary(&self.path);
        let modified = || fs::metadata(&binary).and_then(|meta| meta.modified()).ok();
        let Some(before) = modified() else {
            return Err(Failure::new(
                format!("{} is not there after the copy", binary.display()),
                XTASK_RUN_FIX,
            ));
        };
        let cmd = Cmd::new(Program::Cargo, ["xtask", "--help"], XTASK_TIMEOUT);
        ran(runner, &cmd, &self.path, Where::Worktree)?;
        if modified() == Some(before) {
            return Ok(());
        }
        Err(Failure::new(
            format!(
                "`{cmd}` rebuilt xtask in the worktree: {} changed",
                binary.display()
            ),
            XTASK_REBUILT_FIX,
        ))
    }

    /// After a run: removes the worktree with `git worktree remove --force` when this run's
    /// `git worktree add` succeeded (`added`), then makes sure it is gone from disk and from
    /// `git worktree list`. Nothing was at its path right before the add (`refuse_leftover`), so
    /// after a successful add what is there this run made. When the add failed, xtask removes
    /// nothing: git undoes a failed add itself, and xtask cannot tell whether git or someone else
    /// made what is left, so it only names it.
    pub(super) fn remove(&self, added: bool, runner: &mut QuietRunner<'_>) -> Result<(), Failure> {
        let mut why = Vec::new();
        if added {
            let path = self.path.to_string_lossy();
            let remove = ["worktree", "remove", "--force", &path];
            let cmd = Cmd::new(Program::Git, remove, REMOVE_TIMEOUT);
            if let Err(failure) = ran(runner, &cmd, self.root, Where::Checkout) {
                why.push(with_output(failure));
            }
        }
        match self.listed(runner) {
            Ok(true) => why.push("`git worktree list` names it".to_owned()),
            Ok(false) => {}
            Err(failure) => why.push(failure.message),
        }
        if fs::symlink_metadata(&self.path).is_ok() {
            why.push("its folder is on disk".to_owned());
        }
        match (why.is_empty(), added) {
            (true, _) => Ok(()),
            (false, true) => Err(self.cannot_remove(&why.join("; "))),
            (false, false) => Err(self.left_after_failed_add(&why.join("; "))),
        }
    }

    /// Whether `git worktree list --porcelain` names the worktree.
    pub(super) fn listed(&self, runner: &mut QuietRunner<'_>) -> Result<bool, Failure> {
        let list = ["worktree", "list", "--porcelain"];
        let cmd = Cmd::new(Program::Git, list, GIT_TIMEOUT);
        let listed = ran(runner, &cmd, self.root, Where::Checkout)?;
        Ok(listed
            .stdout
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .any(|path| same_path(Path::new(path), &self.path)))
    }

    /// The failure of what is at the worktree's path after this run's `git worktree add` failed.
    pub(super) fn left_after_failed_add(&self, why: &str) -> Failure {
        let path = self.path.display();
        Failure::new(
            format!(
                "this run's `git worktree add` failed and something is left at {path}: {why}. \
                 xtask cannot tell whether git or someone else made it, so it leaves it as it is"
            ),
            format!(
                "check what is at {path} before you move it away: if `git worktree list` names \
                 it, `git worktree remove {path}` removes it unless it holds modified or \
                 untracked files, and `git worktree prune` forgets it once its folder is gone; \
                 then run again"
            ),
        )
    }

    /// The failure of a worktree this run made and could not remove.
    pub(super) fn cannot_remove(&self, why: &str) -> Failure {
        let path = self.path.display();
        Failure::new(
            format!("could not remove the worktree {path} that this run made: {why}"),
            format!(
                "remove it by hand with `git worktree remove --force {path}`, or delete the \
                 folder if git does not know it (this run made it: it holds a checkout of HEAD \
                 and copied build output, nothing else); run `git worktree prune` if the folder \
                 is already gone; then run again"
            ),
        )
    }
}

/// A failure's message, with what the command printed on one line after it in brackets when it
/// printed anything.
pub(crate) fn with_output(failure: Failure) -> String {
    match failure.output.as_deref().map(one_line) {
        Some(said) if !said.is_empty() => format!("{} ({said})", failure.message),
        _ => failure.message,
    }
}

/// Whether `a` and `b` name the same folder: equal as paths (so `C:/x` and `C:\x` on Windows), or
/// resolving to the same folder.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (dunce::canonicalize(a), dunce::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// xtask's binary in the alias's target dir under `root`.
pub(crate) fn xtask_binary(root: &Path) -> PathBuf {
    root.join("target/xtask-run/debug")
        .join(format!("xtask{}", std::env::consts::EXE_SUFFIX))
}

/// Where a command runs: in the checkout, or in the worktree, which is gone once the run ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Where {
    Checkout,
    Worktree,
}

/// Runs `cmd` in `dir`. `Err` when it cannot start, runs past its limit or exits non-zero; the
/// failure carries what it printed, its JSON messages as people read them and without the
/// `Fresh` and `Running` lines of `-v`. A command that failed in the worktree cannot be run
/// again there, since the run removes it: its fix points at that output, or, for a command past
/// its limit, which printed nothing, says to run it in a worktree made the same way.
fn ran(runner: &mut QuietRunner<'_>, cmd: &Cmd, dir: &Path, place: Where) -> Result<Ran, Failure> {
    match runner(cmd, dir) {
        Ok(done) if done.success() => Ok(done),
        Ok(done) if done.status == Status::TimedOut => Err(Failure::new(
            process::timeout_message(cmd),
            match place {
                Where::Checkout => TimeoutFix::of(cmd).to_string(),
                Where::Worktree => {
                    format!(
                        "in a worktree made the same way at {}, {}",
                        dir.display(),
                        TimeoutFix::of(cmd)
                    )
                }
            },
        )),
        Ok(done) => {
            let mut stderr = String::new();
            for line in done.stderr.lines() {
                let verbose = line.trim_start();
                if !verbose.starts_with("Fresh ") && !verbose.starts_with("Running `") {
                    stderr.push_str(line);
                    stderr.push('\n');
                }
            }
            Err(Failure {
                message: format!("`{cmd}` failed"),
                fix: match place {
                    Where::Checkout => format!("run `{cmd}` from the repo root to see the error"),
                    Where::Worktree => "read its output above (the worktree is removed once the \
                                        run ends), fix what it names, then run again"
                        .to_owned(),
                },
                output: Some(format!("{}{stderr}", clippy_render(&done.stdout))),
            })
        }
        Err(error) => Err(Failure::new(
            format!("could not run `{cmd}`: {error}"),
            format!(
                "make sure `{}` runs from a terminal in {}, then run again",
                cmd.program.name(),
                dir.display()
            ),
        )),
    }
}
