//! `RetryProtection` and `RemoveSandboxSetup` (decision record 0051, R17 PR-B1): the set-up,
//! its removal, or a check, each on a thread of its own (`clauseo-sandbox-setup`), never the
//! actor's. A plain thread, not the launch prep's pool: the set-up can wait at an
//! administrator prompt for minutes.
//!
//! - A command's answer is held until its work ends; no receipt is stored.
//! - A second `RetryProtection` while a set-up or a check runs joins it: the same answer, and no
//!   second run. A second `RemoveSandboxSetup` joins a removal the same way. One kind while the
//!   other runs is `command.invalid` at once.
//! - After the work the thread takes one bounded snapshot (`snapshot.rs`: the detection, the
//!   set-up's state and its time, joining a snapshot thread still out); the actor takes it into
//!   its cache, sends the view and the matters' `ProtectionChanged`, then answers.
//! - With launchers, a first snapshot that gave no answer gets one automatic check
//!   ([`recheck`]), which no command waits on and a `RetryProtection` joins.
//! - Shutdown never waits for the thread (the elevated step is a process of its own): the held
//!   answers get `Closed`, and the thread's late result is dropped.

use clauseo_protocol::{CommandId, CommandOutcome, ErrorCode, ProtectionWork};
use clauseo_sandbox::{SandboxSetup, SetupError};

use std::time::Duration;

use super::{Observed, observe_within};
use crate::actor::Actor;
use crate::actor::commands::CommandCtx;
use crate::actor::input::Input;
use crate::config::Sandbox;
use crate::error::Error;

/// The thread's name.
const THREAD_NAME: &str = "clauseo-sandbox-setup";

/// Which command asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::actor) enum Asked {
    /// `RetryProtection`.
    Retry,
    /// `RemoveSandboxSetup`.
    Remove,
}

/// The work running and the commands waiting for its end.
pub(crate) struct Running {
    id: u64,
    work: ProtectionWork,
    waiters: Vec<CommandId>,
}

impl Running {
    /// What runs.
    pub(super) fn work(&self) -> ProtectionWork {
        self.work
    }
}

/// What the thread posts back.
pub(crate) struct Done {
    /// The work's id.
    id: u64,
    ended: Ended,
}

/// How the work ended.
enum Ended {
    /// It ran: the set-up's or the removal's result (none for a check), then the detection.
    Ran {
        outcome: Option<Result<(), SetupError>>,
        observed: Observed,
    },
    /// The thread ended without a result (it panicked): the cache stays as it was.
    Lost,
}

/// A command's answer, built once per waiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Done,
    Refused,
    Failed,
}

impl Answer {
    fn result(self) -> Result<CommandOutcome, Error> {
        match self {
            Self::Done => Ok(CommandOutcome::Done),
            Self::Refused => Err(Error::refused(
                ErrorCode::SandboxSetupRefused,
                "the administrator prompt was declined",
            )),
            Self::Failed => Err(Error::refused(
                ErrorCode::SandboxSetupFailed,
                "the sandbox set-up did not finish with a working sandbox",
            )),
        }
    }
}

/// `RetryProtection` or `RemoveSandboxSetup`: joins the running work, refuses, or starts it.
pub(in crate::actor) fn command(actor: &mut Actor, ctx: &CommandCtx, asked: Asked) {
    if let Err(error) = begin(actor, ctx.id, asked) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn begin(actor: &mut Actor, id: CommandId, asked: Asked) -> Result<(), Error> {
    let host = &mut actor.protection;
    let Some(sandbox) = host.sandbox.clone() else {
        return Err(invalid("this build of Clauseo has no sandbox"));
    };
    if let Some(running) = &mut host.running {
        let joins = match running.work {
            ProtectionWork::SetUp | ProtectionWork::Check => asked == Asked::Retry,
            ProtectionWork::Remove => asked == Asked::Remove,
            ProtectionWork::Unknown => false,
        };
        if !joins {
            return Err(invalid(
                "the sandbox's set-up or its removal is running; try again when it ends",
            ));
        }
        running.waiters.push(id);
        return Ok(());
    }
    let work = match asked {
        Asked::Retry if host.observed.asks_admin => ProtectionWork::SetUp,
        Asked::Retry => ProtectionWork::Check,
        Asked::Remove => ProtectionWork::Remove,
    };
    if work == ProtectionWork::Remove && sandbox.setup.is_none() {
        return Err(invalid("this computer's sandbox has no set-up to remove"));
    }
    let wait = sandbox.bounds.detect_wait;
    start(actor, sandbox, work, vec![id], wait)
}

/// The one automatic check after a first snapshot that gave no answer (`protection::start`,
/// with launchers): no command waits on it, and its snapshot waits `retry_wait`, joining the
/// first one's thread if it is still out. A `RetryProtection` meanwhile joins it.
pub(super) fn recheck(actor: &mut Actor) {
    let Some(sandbox) = actor.protection.sandbox.clone() else {
        return;
    };
    if actor.protection.running.is_some() {
        return;
    }
    let wait = sandbox.bounds.retry_wait;
    if let Err(error) = start(actor, sandbox, ProtectionWork::Check, Vec::new(), wait) {
        tracing::warn!(code = ?error.code(), "the sandbox's automatic check did not start");
    }
}

/// Starts `work` on its thread for `waiters`, its snapshot afterwards waiting at most `wait`.
fn start(
    actor: &mut Actor,
    sandbox: Sandbox,
    work: ProtectionWork,
    waiters: Vec<CommandId>,
    wait: Duration,
) -> Result<(), Error> {
    let host = &mut actor.protection;
    let setup = sandbox.setup.clone();
    let work_id = host.next_work;
    host.next_work += 1;
    let post = actor.post.clone();
    let report = Report {
        post: Some(post),
        id: work_id,
    };
    #[cfg(feature = "test-support")]
    let fail = sandbox.fail_spawn;
    #[cfg(not(feature = "test-support"))]
    let fail = false;
    let data_dir = actor.effective.data_dir().to_path_buf();
    let spawned = spawn(fail, move || {
        let outcome = run(work, setup.as_deref());
        if work == ProtectionWork::Remove {
            // A snapshot from before the removal may still say set up: a fresh one decides.
            sandbox.bounds.slot.removed();
        }
        let observed = observe_within(Some(&sandbox), &data_dir, wait);
        report.send(Ended::Ran { outcome, observed });
    });
    if let Err(error) = spawned {
        tracing::warn!(error = %crate::log_file::Cause(&error), "the sandbox thread did not start");
        return Err(Error::refused(
            ErrorCode::SandboxSetupFailed,
            "the sandbox set-up could not start",
        ));
    }
    actor.protection.running = Some(Running {
        id: work_id,
        work,
        waiters,
    });
    tracing::info!(work = work_name(work), "the sandbox work started");
    super::publish_view(actor);
    Ok(())
}

/// Starts the work's thread; `fail` (a test's seam, `Sandbox::failing_spawn`) makes it fail
/// as the OS would.
fn spawn(fail: bool, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    if fail {
        return Err(std::io::Error::other("a test's seam refused the thread"));
    }
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(body)
        .map(drop)
}

/// The work itself, on the thread: the set-up's run, its removal, or nothing for a check.
fn run(work: ProtectionWork, setup: Option<&dyn SandboxSetup>) -> Option<Result<(), SetupError>> {
    match work {
        ProtectionWork::SetUp => setup.map(SandboxSetup::run),
        // A removal ends what still runs as a pool user (tier B's sessions). While it runs the
        // cache still says `Protected { tier: B }`, so launches through the sandbox's launchers
        // are refused with `sandbox.removing` (`protection::refusal`).
        ProtectionWork::Remove => setup.map(SandboxSetup::remove),
        ProtectionWork::Check | ProtectionWork::Unknown => None,
    }
}

/// Posts the thread's end once, or, if the thread panics first, [`Ended::Lost`] from `Drop`,
/// so no command waits on a thread that is gone. That holds where panics unwind (debug and test
/// builds); the release profile sets `panic = "abort"`, so there a panic in `SandboxSetup`
/// ends the app, and the seam must not panic.
struct Report {
    post: Option<async_channel::Sender<Input>>,
    id: u64,
}

impl Report {
    fn send(mut self, ended: Ended) {
        if let Some(post) = self.post.take() {
            // The host may have stopped; nobody is left to tell.
            let _gone = post.try_send(Input::Protection(Box::new(Done { id: self.id, ended })));
        }
    }
}

impl Drop for Report {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            let done = Done {
                id: self.id,
                ended: Ended::Lost,
            };
            let _gone = post.try_send(Input::Protection(Box::new(done)));
        }
    }
}

/// The thread's end: the cache, the view, the matters' rows, then the held answers. A result
/// of work no longer running (shutdown answered it) is dropped, as is any while stopping. One
/// that ends while the host still starts (the automatic check, [`recheck`]) is taken.
pub(in crate::actor) fn on_done(actor: &mut Actor, done: Done) {
    let Done { id, ended } = done;
    let serving = !matches!(actor.phase, crate::actor::HostPhase::Stopping(_));
    let current = actor
        .protection
        .running
        .as_ref()
        .is_some_and(|r| r.id == id);
    if !serving || !current {
        return;
    }
    let Some(running) = actor.protection.running.take() else {
        return;
    };
    let answer = match ended {
        Ended::Lost => {
            tracing::warn!(
                work = work_name(running.work),
                "the sandbox thread ended early"
            );
            super::publish_view(actor);
            Answer::Failed
        }
        Ended::Ran { outcome, observed } => {
            let changed = observed.computer != actor.protection.observed.computer;
            actor.protection.observed = observed;
            super::publish_view(actor);
            if changed {
                super::matters_changed(actor);
            }
            let protected = actor.protection.observed.sandbox_protected();
            answer(running.work, outcome, protected)
        }
    };
    tracing::info!(
        work = work_name(running.work),
        ?answer,
        "the sandbox work ended"
    );
    for waiter in running.waiters {
        actor.finish_command(waiter, answer.result());
    }
}

/// The answer to `work` that ended with `outcome`, the computer then `protected` or not.
fn answer(
    work: ProtectionWork,
    outcome: Option<Result<(), SetupError>>,
    protected: bool,
) -> Answer {
    match (work, outcome) {
        (_, Some(Err(SetupError::Refused))) => Answer::Refused,
        (_, Some(Err(SetupError::Failed(_)))) => {
            // What failed may name a path or an account: its kind only (0047).
            tracing::warn!(
                work = work_name(work),
                kind = "failed",
                "a sandbox step failed"
            );
            Answer::Failed
        }
        (_, Some(Err(SetupError::Blocked(_)))) => {
            // A Windows policy refused a right the set-up needs (stage C's D-B4); R17 PR-E gives
            // it a named cause. Its kind only (0047).
            tracing::warn!(
                work = work_name(work),
                kind = "blocked",
                "a sandbox step was blocked by a Windows policy"
            );
            Answer::Failed
        }
        (ProtectionWork::SetUp, Some(Ok(()))) if !protected => Answer::Failed,
        (ProtectionWork::SetUp | ProtectionWork::Remove, Some(Ok(())))
        | (ProtectionWork::Check | ProtectionWork::Unknown, _) => Answer::Done,
        (ProtectionWork::SetUp | ProtectionWork::Remove, None) => Answer::Failed,
    }
}

/// Shutdown: the held answers get `Closed`, and the thread is left to end on its own.
pub(in crate::actor) fn shutdown(actor: &mut Actor) {
    if let Some(running) = actor.protection.running.take() {
        for waiter in running.waiters {
            actor.finish_command(waiter, Err(Error::Closed));
        }
    }
}

/// The log's name of a work.
fn work_name(work: ProtectionWork) -> &'static str {
    match work {
        ProtectionWork::SetUp => "set_up",
        ProtectionWork::Remove => "remove",
        ProtectionWork::Check => "check",
        ProtectionWork::Unknown => "unknown",
    }
}

/// `command.invalid` with `message`.
fn invalid(message: &str) -> Error {
    Error::refused(ErrorCode::CommandInvalid, message)
}
