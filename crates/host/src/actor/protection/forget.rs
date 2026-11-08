//! A forgotten matter's sandbox state (R17 PR-E): once `ForgetMatter` is durable, the set-up
//! drops the matter's permission entries and its pool user's claim
//! (`SandboxSetup::forget_matter`), so the pool user can go to another matter. It reads and
//! writes the sandbox's records and the matter's folders, so it runs off the actor, on the
//! walker's thread (`forget/threads.rs`).
//!
//! - Owed: a durable forget is first recorded as owed in
//!   `<data_dir>/protection/owed_forgets.json` (matter ids only, `forget/threads.rs`), by the
//!   writer's thread; only once that write is back may its walk start. A walk that succeeds
//!   drops the id; one that fails keeps it (logged by its kind only, 0047).
//! - Agents first: the walk waits until no thread of the matter has an agent or a launch being
//!   prepared (`turn::agents_gone`). `ForgetMatter` stops the matter's agents
//!   (`turn::stop_agents`), and their stop ladders always end in `Ended`; [`tick`], after every
//!   input, sees the last one gone and starts the walk.
//! - Replay: at each start the writer reads the file and every forget still owed (a failure,
//!   or a quit before it ended) is walked again; a forgotten matter has no agents at start. A
//!   matter back on the list (an import brought it back) is dropped from the file instead,
//!   never walked.
//! - Stop: no walk starts once the stop began. A walk or a write still running is waited on
//!   for at most [`STOP_WAIT`] on the host clock (as the allowed sites' saves are); past it
//!   both threads write and walk nothing more, and what is owed stays for the next start.
//! - With no set-up (macOS, Linux, no seam) nothing is recorded or walked.
//! - Residual: a walk already running when its matter comes back (an import in the same few
//!   moments) still ends; the next launch in the matter claims a pool user again.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use clauseo_protocol::MatterId;

pub(crate) use self::threads::Report;
use self::threads::{Job, Write};
use crate::actor::input::Input;
use crate::actor::{Actor, HostPhase, turn};

mod threads;

/// How long the host's stop waits for a walk or a write still running.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(2);

/// What reaches the actor about the forgets.
#[derive(Debug)]
pub(crate) enum ForgetInput {
    /// From a thread.
    Report(Report),
    /// [`STOP_WAIT`] passed since the stop began, or the threads were done first.
    StopWaited,
}

/// The forgets on the actor.
#[derive(Debug, Default)]
pub(crate) struct Forgets {
    /// The writer's queue; `None` with no set-up, or once its thread is gone.
    writer: Option<mpsc::Sender<Job>>,
    /// The walker's queue; `None` likewise.
    walker: Option<mpsc::Sender<MatterId>>,
    /// Owed matters whose walk waits for their agents to be gone.
    waiting: BTreeSet<MatterId>,
    /// Writes sent and not yet back.
    writes: usize,
    /// Walks sent and not yet back.
    walks: usize,
    /// While the stop waits for the threads: dropping it ends the wait's timer.
    stop_wait: Option<futures::channel::oneshot::Sender<()>>,
    /// Set once the stop gave up on the threads: they write and walk nothing more.
    abandoned: Arc<AtomicBool>,
}

/// At the start, with a set-up: the writer, which reads the file first, and the walker.
pub(in crate::actor) fn start(actor: &mut Actor) {
    let Some(setup) = actor
        .protection
        .sandbox
        .as_ref()
        .and_then(|sandbox| sandbox.setup.clone())
    else {
        return;
    };
    let path = actor
        .effective
        .data_dir()
        .join(threads::FOLDER)
        .join(threads::FILE);
    let abandoned = Arc::clone(&actor.protection.forgets.abandoned);
    let (writer, writes) = mpsc::channel::<Job>();
    let post = actor.post.clone();
    let stop = Arc::clone(&abandoned);
    let started = std::thread::Builder::new()
        .name(threads::WRITER.to_owned())
        .spawn(move || threads::write_owed(&path, &stop, &writes, &|report| send(&post, report)));
    match started {
        Ok(_) => actor.protection.forgets.writer = Some(writer),
        Err(error) => tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "no thread for the sandbox's owed forgets; forgets made now are not retried"
        ),
    }
    let (walker, matters) = mpsc::channel::<MatterId>();
    let post = actor.post.clone();
    let started = std::thread::Builder::new()
        .name(threads::WALKER.to_owned())
        .spawn(move || threads::walk(&*setup, &abandoned, &matters, &|report| send(&post, report)));
    match started {
        Ok(_) => actor.protection.forgets.walker = Some(walker),
        Err(error) => tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "no thread for the sandbox's forgets; they stay owed for the next start"
        ),
    }
}

/// Posts a thread's report; the host may have stopped, and nobody is left to tell.
fn send(post: &async_channel::Sender<Input>, report: Report) {
    let input = ForgetInput::Report(report);
    let _gone = post.try_send(Input::SandboxForget(Box::new(input)));
}

/// `matter` was forgotten (its commit is durable): recorded as owed, then walked once that
/// write is back and its agents are gone.
pub(in crate::actor) fn matter_forgotten(actor: &mut Actor, matter: MatterId) {
    let now = actor.now().wall;
    let forgets = &mut actor.protection.forgets;
    if let Some(writer) = &forgets.writer {
        let job = Job {
            write: Write::Owe(matter),
            now,
        };
        if writer.send(job).is_ok() {
            forgets.writes += 1;
            return;
        }
        forgets.writer = None;
    }
    // No writer: walked all the same, though not retried when it fails.
    if forgets.walker.is_some() {
        forgets.waiting.insert(matter);
    }
}

/// A [`ForgetInput`] on the actor.
pub(in crate::actor) fn on_input(actor: &mut Actor, input: ForgetInput) {
    let forgets = &mut actor.protection.forgets;
    match input {
        ForgetInput::Report(Report::Read(owed)) => {
            if !owed.is_empty() {
                tracing::info!(owed = owed.len(), "the sandbox's owed forgets are replayed");
            }
            forgets.waiting.extend(owed);
        }
        ForgetInput::Report(Report::Owed(matter)) => {
            forgets.writes = forgets.writes.saturating_sub(1);
            forgets.waiting.insert(matter);
        }
        ForgetInput::Report(Report::Dropped) => {
            forgets.writes = forgets.writes.saturating_sub(1);
        }
        ForgetInput::Report(Report::Walked { matter, forgot }) => {
            forgets.walks = forgets.walks.saturating_sub(1);
            if forgot {
                write(actor, Write::Drop(matter));
            }
        }
        ForgetInput::StopWaited => {
            if forgets.stop_wait.take().is_some() {
                forgets.abandoned.store(true, Ordering::SeqCst);
                tracing::warn!(
                    wait = ?STOP_WAIT,
                    "the sandbox's forget was still running at the stop; it stays owed"
                );
            }
            return;
        }
    }
    let forgets = &mut actor.protection.forgets;
    if forgets.writes == 0 && forgets.walks == 0 {
        // Ends the stop's wait, if any: its timer posts a `StopWaited` that is then ignored.
        forgets.stop_wait = None;
    }
}

/// Sends `write` to the writer, counted until it is back.
fn write(actor: &mut Actor, write: Write) {
    let now = actor.now().wall;
    let forgets = &mut actor.protection.forgets;
    let Some(writer) = &forgets.writer else {
        return;
    };
    if writer.send(Job { write, now }).is_ok() {
        forgets.writes += 1;
    } else {
        forgets.writer = None;
    }
}

/// After every input: each owed matter whose agents are gone is walked, and one back on the
/// list is dropped from the file instead. Nothing starts once the stop began.
pub(in crate::actor) fn tick(actor: &mut Actor) {
    if actor.protection.forgets.waiting.is_empty() || matches!(actor.phase, HostPhase::Stopping(_))
    {
        return;
    }
    let ready: Vec<MatterId> = actor
        .protection
        .forgets
        .waiting
        .iter()
        .copied()
        .filter(|matter| actor.matters.contains_key(matter) || turn::agents_gone(actor, *matter))
        .collect();
    for matter in ready {
        actor.protection.forgets.waiting.remove(&matter);
        if actor.matters.contains_key(&matter) {
            write(actor, Write::Drop(matter));
            continue;
        }
        let forgets = &mut actor.protection.forgets;
        let Some(walker) = &forgets.walker else {
            continue;
        };
        if walker.send(matter).is_ok() {
            forgets.walks += 1;
        } else {
            forgets.walker = None;
        }
    }
}

/// At the host's stop: waits up to [`STOP_WAIT`] for a walk or a write still running, if any.
pub(in crate::actor) fn shutdown(actor: &mut Actor) {
    let forgets = &actor.protection.forgets;
    if (forgets.writes == 0 && forgets.walks == 0) || forgets.stop_wait.is_some() {
        return;
    }
    let deadline = actor.clock.after(STOP_WAIT);
    let (stop, stopped) = futures::channel::oneshot::channel::<()>();
    actor.protection.forgets.stop_wait = Some(stop);
    actor.spawn_post(async move {
        let _either = futures::future::select(deadline, stopped).await;
        Input::SandboxForget(Box::new(ForgetInput::StopWaited))
    });
}

/// Whether the stop still waits on a walk or a write.
pub(in crate::actor) fn busy(actor: &Actor) -> bool {
    actor.protection.forgets.stop_wait.is_some()
}
