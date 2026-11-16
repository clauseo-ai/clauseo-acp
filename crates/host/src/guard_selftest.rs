//! The launch self-test of a guard's hook, every guard's: before a launch, the host runs the
//! exact hook command the agent will run (program and arguments from the hook entry's JSON,
//! [`command_of`]) on a made-up input the guard must refuse, and refuses the launch unless it
//! refuses within [`LIMIT`]: exit 2 with a reason on stderr ([`run`]). The refusal is
//! `ErrorCode::AgentGuardSelftestFailed`
//! (`U3-err-agent.guard_selftest_failed`). The file fence's input and its call are
//! `file_fence::selftest`.
//!
//! This file is shared with the file-write guard (branch `file-write-guard`, commit 351617995):
//! launch-never-weaker (#1167) carries a copy, and whichever of the two merges second drops its
//! copy at rebase. On top of that copy, which the file guard takes at its rebase: [`Spawn`] and
//! [`run_as`] (how the agent spawns the hook, stated, never guessed; [`run`] is
//! `run_as(Spawn::ClaudeExec, …)`), and one more try after a [`Failed::Late`] ([`ask_twice`]:
//! the second start is warm). The pre-write guard's input and its call are
//! `pre_write_guard::selftest`. Each has an `_until` sibling ([`run_until`], [`run_as_until`],
//! [`ask_twice_until`], [`ask_until`]) that gives the self-test up as soon as a stop signal ends
//! ([`Failed::Stopped`]), ending the hook's process tree: the lawyer's Stop of a launch waiting
//! on it (`actor/turn/launch_checks.rs`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, Timer};
use clauseo_protocol::ErrorCode;
use futures::FutureExt as _;
use futures::future::{Either, join, pending, select};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use serde_json::Value;

use crate::clock::{Clock, ProcessTimer};
use crate::error::Error;

/// How long the hook may take to refuse, process start included, each of its two tries. Measured
/// on the laptop, 16 Nov (Win 11 26H2, Defender real-time on): cold right after a reboot
/// 3,106 ms; cold later 484 ms; warm ~16 ms. 10 s is about 3x the worst seen, because a lawyer's
/// PC can be slower (an older disk, third-party antivirus). A long limit costs nothing normally
/// (the wait is the hook's real run) and only delays refusing a truly hung guard. It stays far
/// under the hook's 600 s timeout (`agent::loads::GUARD_TIMEOUT_SECONDS`); the guard's own 30 s
/// watchdog (`pre_write_guard/watchdog.rs`) is past it, so a guard hung before its answer is late
/// here, not refused there.
pub(crate) const LIMIT: Duration = Duration::from_secs(10);

/// The most stdout read.
const MAX_OUTPUT: u64 = 64 * 1024;

/// The words of `U3-err-agent.guard_selftest_failed`, provisional (`"hi": null`): `{agent}`
/// filled.
pub(crate) const GUARD_SELFTEST_FAILED: &str = "Clauseo couldn't start its file check, so \
     {agent} didn't start. Quit and reopen Clauseo, then try again.";

/// Why the self-test failed, for the log (no path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failed {
    /// No hook entry could be built (no program, a path that is not UTF-8).
    NoEntry,
    /// The program could not be started.
    Launch,
    /// It did not answer within [`LIMIT`].
    Late,
    /// It answered, but did not refuse.
    Answer,
    /// Its launch was given up before it answered (the lawyer's Stop, [`run_until`]): the hook's
    /// process tree was ended, and nothing is refused.
    Stopped,
}

/// The program and arguments of a hook entry (`agent::loads::guard_entry`'s JSON).
pub(crate) fn command_of(entry: &Value) -> Option<(PathBuf, Vec<OsString>)> {
    let hook = entry.get("hooks")?.get(0)?;
    let program = PathBuf::from(hook.get("command")?.as_str()?);
    let args = hook
        .get("args")?
        .as_array()?
        .iter()
        .map(|arg| arg.as_str().map(OsString::from))
        .collect::<Option<Vec<_>>>()?;
    Some((program, args))
}

/// Runs the hook `entry` on `input` from `cwd`, timed by `clock`: `Ok` when it refused
/// within [`LIMIT`] (exit 2, a reason on stderr). Blocks the calling thread (the launch prep's).
/// Claude Code's spawn: [`run_as`] with [`Spawn::ClaudeExec`].
pub(crate) fn run(
    entry: Option<&Value>,
    input: &[u8],
    cwd: &Path,
    clock: &Arc<dyn Clock>,
) -> Result<(), Failed> {
    run_as(Spawn::ClaudeExec, entry, input, cwd, clock)
}

/// [`run`], given up as soon as `stop` ends ([`Failed::Stopped`]): the hook's process tree is
/// ended then, whichever try is running, so a launch the lawyer stopped leaves no self-test
/// running for up to two of [`LIMIT`].
pub(crate) fn run_until(
    entry: Option<&Value>,
    input: &[u8],
    cwd: &Path,
    clock: &Arc<dyn Clock>,
    stop: impl Future<Output = ()>,
) -> Result<(), Failed> {
    run_as_until(Spawn::ClaudeExec, entry, input, cwd, clock, stop)
}

/// How the agent spawns a hook entry's command, stated by the caller and never guessed from
/// the entry: the self-test must start the process exactly as the agent will.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spawn {
    /// Claude Code 2.1.293's exec form: the entry's `command` started directly with its `args`
    /// ([`command_of`]), no shell, the hook input as JSON on stdin, from the session's folder.
    /// An entry without `args` is no entry here ([`Failed::NoEntry`]): Claude Code would run it
    /// through a shell, which this does not imitate.
    ClaudeExec,
    // Room for Codex's Windows spawn (`powershell.exe -NoProfile -Command "<commandWindows>"`,
    // or `cmd.exe /C`; env cleared then its snapshot replayed; codex-rs 0.162.1
    // `hooks/src/engine/command_runner.rs:384-433`), once it is confirmed.
}

/// [`run`] for a hook the agent spawns as `spawn` says: the command taken from `entry` as
/// that spawn reads it, one more try after a [`Failed::Late`] ([`ask_twice`]).
pub(crate) fn run_as(
    spawn: Spawn,
    entry: Option<&Value>,
    input: &[u8],
    cwd: &Path,
    clock: &Arc<dyn Clock>,
) -> Result<(), Failed> {
    let (program, args) = match spawn {
        Spawn::ClaudeExec => entry.and_then(command_of).ok_or(Failed::NoEntry)?,
    };
    let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
    let timer = ProcessTimer(Arc::clone(clock));
    let command = (program, args, cwd.to_path_buf());
    futures::executor::block_on(ask_twice(&launcher, &timer, command, input))
}

/// [`run_as`], given up as soon as `stop` ends, as [`run_until`] says.
pub(crate) fn run_as_until(
    spawn: Spawn,
    entry: Option<&Value>,
    input: &[u8],
    cwd: &Path,
    clock: &Arc<dyn Clock>,
    stop: impl Future<Output = ()>,
) -> Result<(), Failed> {
    let (program, args) = match spawn {
        Spawn::ClaudeExec => entry.and_then(command_of).ok_or(Failed::NoEntry)?,
    };
    let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
    let timer = ProcessTimer(Arc::clone(clock));
    let command = (program, args, cwd.to_path_buf());
    futures::executor::block_on(ask_twice_until(&launcher, &timer, command, input, stop))
}

/// [`ask`], and once more when it was [`Failed::Late`]: a first start can be slow for reasons
/// that pass (a cold disk, security software scanning the exe once), and the second start is
/// warm. Any other failure is final at once.
pub(crate) async fn ask_twice<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    command: (PathBuf, Vec<OsString>, PathBuf),
    input: &[u8],
) -> Result<(), Failed> {
    match ask(launcher, timer, command.clone(), input).await {
        Err(Failed::Late) => {
            tracing::info!("a guard's self-test was late once: trying it once more");
            ask(launcher, timer, command, input).await
        }
        answered => answered,
    }
}

/// [`ask_twice`], given up as soon as `stop` ends, during either try ([`Failed::Stopped`]).
pub(crate) async fn ask_twice_until<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    command: (PathBuf, Vec<OsString>, PathBuf),
    input: &[u8],
    stop: impl Future<Output = ()>,
) -> Result<(), Failed> {
    let mut stop = pin!(stop.fuse());
    match ask_until(launcher, timer, command.clone(), input, stop.as_mut()).await {
        Err(Failed::Late) => {
            tracing::info!("a guard's self-test was late once: trying it once more");
            ask_until(launcher, timer, command, input, stop).await
        }
        answered => answered,
    }
}

/// [`run`]'s process: started by `launcher`, `input` on its stdin, timed by `timer`.
pub(crate) async fn ask<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    command: (PathBuf, Vec<OsString>, PathBuf),
    input: &[u8],
) -> Result<(), Failed> {
    ask_until(launcher, timer, command, input, pin!(pending::<()>())).await
}

/// [`ask`], given up as soon as `stop` ends ([`Failed::Stopped`]): the handle drops, which
/// ends the hook's process tree.
pub(crate) async fn ask_until<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    (program, args, cwd): (PathBuf, Vec<OsString>, PathBuf),
    input: &[u8],
    stop: impl Future<Output = ()> + Unpin,
) -> Result<(), Failed> {
    let spec =
        LaunchSpec::new(program, args, BTreeMap::new(), cwd).map_err(|_invalid| Failed::Launch)?;
    let Launched {
        mut stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|_unstarted| Failed::Launch)?;
    let mut out = Vec::new();
    let work = async {
        // A hook that stops reading early answers anyway; the exit code decides.
        drop(stdin.write_all(input).await);
        drop(stdin.close().await);
        // Bound to a name: the `Take` must outlive the read it lends itself to.
        let mut limited = stdout.take(MAX_OUTPUT);
        let reading = limited.read_to_end(&mut out);
        let (read, _drained) = join(reading, stderr.drain()).await;
        let exit = handle.exited().await;
        read.is_ok().then(|| (exit.code(), exit.stderr_tail()))
    };
    let answered = match select(select(pin!(work), pin!(timer.after(LIMIT))), stop).await {
        Either::Left((Either::Left((answered, _)), _)) => answered,
        Either::Left((Either::Right(((), _)), _)) => return Err(Failed::Late),
        // The handle drops on the way out, which ends the hook's tree.
        Either::Right(((), _)) => return Err(Failed::Stopped),
    };
    // Dropping the handle ends the hook's tree, if anything of it is left.
    drop(handle);
    let refused = answered.is_some_and(|(code, errors)| {
        // Any refusal with a reason passes: a hook that refuses this call for another of its
        // checks fails closed too.
        code == Some(crate::pre_write_guard::DENY) && !errors.trim().is_empty()
    });
    if refused { Ok(()) } else { Err(Failed::Answer) }
}

/// The launch's refusal for a failed self-test of `agent`'s guard (`failed`, logged by kind).
pub(crate) fn refusal(failed: Failed, agent: &str) -> Error {
    tracing::warn!(?failed, "a guard's self-test failed: the launch is refused");
    Error::refused(
        ErrorCode::AgentGuardSelftestFailed,
        GUARD_SELFTEST_FAILED.replace("{agent}", agent),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Arc;

    use clauseo_process::{PassthroughLauncher, SpawnOptions};

    use super::{Failed, ask, ask_twice, command_of};
    use crate::clock::{Clock, ProcessTimer, SystemClock};

    /// The self-test against `sh -c script`.
    fn run_script(script: &str) -> Result<(), Failed> {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let timer = ProcessTimer(clock);
        let args = vec![OsString::from("-c"), OsString::from(script)];
        let command = (PathBuf::from("/bin/sh"), args, std::env::temp_dir());
        futures::executor::block_on(ask(&launcher, &timer, command, b"{}"))
    }

    #[test]
    fn a_hook_that_refuses_with_a_reason_in_time_passes_and_any_other_answer_fails() {
        let refuse = "cat >/dev/null; echo 'Permission has been denied' >&2; exit 2";
        assert_eq!(run_script(refuse), Ok(()));
        assert_eq!(run_script("cat >/dev/null; echo other >&2; exit 2"), Ok(()));
        assert_eq!(run_script("cat >/dev/null; exit 2"), Err(Failed::Answer));
        assert_eq!(run_script("cat >/dev/null; exit 0"), Err(Failed::Answer));
        assert_eq!(run_script("cat >/dev/null; exit 1"), Err(Failed::Answer));
        assert_eq!(run_script("exec sleep 60"), Err(Failed::Late));
    }

    #[test]
    fn a_missing_program_or_entry_fails() {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
        let timer = ProcessTimer(Arc::new(SystemClock));
        let missing = (
            std::env::temp_dir().join("no-such-guard"),
            Vec::new(),
            std::env::temp_dir(),
        );
        let ran = futures::executor::block_on(ask(&launcher, &timer, missing, b"{}"));
        assert_eq!(ran, Err(Failed::Launch));
        assert_eq!(command_of(&serde_json::json!({"hooks": []})), None);
        let entry =
            serde_json::json!({"hooks": [{"command": "/g", "args": ["--pre-write-guard"]}]});
        let (program, args) = command_of(&entry).unwrap_or_default();
        assert_eq!(
            (program, args),
            (
                PathBuf::from("/g"),
                vec![OsString::from("--pre-write-guard")]
            )
        );
    }

    /// A timer stand-in: its first wait ends at once (that try is late), every later one never
    /// ends (the hook's own answer decides).
    #[derive(Default)]
    struct LateOnce(std::sync::atomic::AtomicUsize);

    impl clauseo_process::Timer for LateOnce {
        fn after(
            &self,
            _duration: std::time::Duration,
        ) -> impl std::future::Future<Output = ()> + Send {
            let first = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
            if first {
                futures::future::Either::Left(futures::future::ready(()))
            } else {
                futures::future::Either::Right(futures::future::pending())
            }
        }
    }

    /// A first try that times out is tried once more, and the second's refusal passes; a hook
    /// that answers wrong is not tried again.
    #[test]
    fn a_late_first_try_is_tried_once_more() {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
        let script = "cat >/dev/null; echo refused >&2; exit 2";
        let command = |script: &str| {
            let args = vec![OsString::from("-c"), OsString::from(script)];
            (PathBuf::from("/bin/sh"), args, std::env::temp_dir())
        };
        let timer = LateOnce::default();
        let ran = futures::executor::block_on(ask_twice(&launcher, &timer, command(script), b"{}"));
        assert_eq!(ran, Ok(()));
        assert_eq!(timer.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        let timer = LateOnce::default();
        let late =
            futures::executor::block_on(ask_twice(&launcher, &timer, command("exit 0"), b"{}"));
        assert_eq!(late, Err(Failed::Answer));
        assert_eq!(timer.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        let timer = LateOnce::default();
        let once = futures::executor::block_on(ask(&launcher, &timer, command(script), b"{}"));
        assert_eq!(once, Err(Failed::Late));
    }

    /// A stop that ends gives the self-test up at once, during its first try, with no second
    /// try: a hook that would hang for a minute ends as `Stopped`.
    #[test]
    fn a_stop_gives_the_self_test_up_at_once() {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
        let timer = ProcessTimer(Arc::new(SystemClock));
        let args = vec![OsString::from("-c"), OsString::from("exec sleep 60")];
        let command = (PathBuf::from("/bin/sh"), args, std::env::temp_dir());
        let stopped = futures::executor::block_on(super::ask_twice_until(
            &launcher,
            &timer,
            command,
            b"{}",
            futures::future::ready(()),
        ));
        assert_eq!(stopped, Err(Failed::Stopped));
    }
}
