//! One agent process and its ACP connection, driven by four tasks on the actor's `LocalPool`:
//! R10's `Driver`, the event forwarder, S7's stderr pump, and the owner task that holds S7's
//! `Handle`. The owner waits for the agent to exit or for a stop; on a stop it drops the `exited`
//! future, lists the agent's descendants on a `clauseo-sweep` thread (on Unix the tree kill ends
//! those that left the agent's group) and runs the shutdown ladder, because `exited` borrows the
//! handle and `shutdown` consumes it. Either way it then waits for the stderr pump and the event
//! forwarder to end and then for every call that took a [`AgentLink::fence`] to post its answer,
//! reads the exit report, and posts [`Ended`] after the agent's last event and after those
//! answers. So a prompt the agent answered just before it exited (a Codex limit error, then the
//! exit) reaches the actor as the prompt's answer, never as the exit, and the last chunk it wrote
//! before it exited is posted before its `Ended`.
//!
//! A helper the agent left holding a pipe would hold that wait open, so it gives up once
//! [`PUMP_END_BOUND`] has passed with all the agent wrote to stdout before the exit posted
//! (`acp`'s [`ReadProgress`], whose mark at the exit makes the reader read again, so a pipe
//! that is only held open is found empty within the first bound), and at the latest after
//! [`PUMP_END_ROUNDS`] bounds. It never gives up on the bound alone: a host thread that ran late
//! (or a test clock moved on) must not leave the agent's last chunk unposted behind its `Ended`.

pub(crate) mod codex;
pub(crate) mod data_folder;
pub(crate) mod folder_support;
pub(crate) mod forced_ask;
pub(crate) mod leftovers;
pub(crate) mod loads;
mod spec;
pub(crate) mod title;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Poll, Waker};
use std::time::Duration;

use clauseo_acp::{AcpClient, AcpEvent, ReadProgress};
use clauseo_process::{Descendants, Handle, LaunchSpec, Launched, Launcher, ShutdownOutcome};
use clauseo_protocol::DetachReason;
use futures::channel::oneshot;
use futures::executor::LocalSpawner;
use futures::future::{Either, join, select};
use futures::task::LocalSpawnExt;

use crate::clock::{Clock, ProcessTimer};
use crate::error::Error;

pub(crate) use spec::launch_spec;

/// How long the owner waits, after the agent is gone, for its stderr pipe and its stdout to end,
/// so the exit report's tail holds the last lines and [`Ended`] follows every event. A helper the
/// agent left holding a pipe, writing nothing, holds the report back this long, or one bound
/// more each time the bound passes with what the agent wrote before it exited still on its way.
pub(crate) const PUMP_END_BOUND: Duration = Duration::from_secs(1);

/// The most bounds the owner waits: past them it posts [`Ended`] even with the agent's stdout
/// still being read, so a helper that keeps writing to it cannot hold the report back for good.
pub(crate) const PUMP_END_ROUNDS: u32 = 10;

/// The one spelling of matter folder `stored` an agent works in, as its process's working
/// folder and its session's `cwd`: its canonical path (`dunce::canonicalize`: symlinks and `..`
/// followed), or `stored` itself when that fails or when the canonical path keeps a Windows
/// `\\?\` prefix ([`keep_canonical`]). So the result never starts with `\\?\` unless `stored`
/// did, and a network folder (a UNC share or a mapped drive, which canonicalises to
/// `\\?\UNC\...`) keeps its stored spelling. The host starts the agent in it and sends it as
/// `cwd` on `session/new` and `session/load`, so it is the folder `acp` maps tool locations
/// against. Claude Code works in the real path of its cwd and names its files through it; sent
/// the folder as the lawyer picked it (macOS `/tmp`, really `/private/tmp`), every tool path of
/// the turn would land outside the matter.
///
/// A blocking file system call (a few `lstat`s): call it once per agent start or session start.
pub fn session_folder(stored: &Path) -> PathBuf {
    match dunce::canonicalize(stored) {
        Ok(canonical) => keep_canonical(&canonical, stored),
        Err(_) => stored.to_path_buf(),
    }
}

/// `canonical`, unless its text starts with Windows' verbatim prefix `\\?\`: then `stored`.
/// `dunce` strips the prefix only where the plain spelling means the same folder (a local drive
/// path), so one that is left (`\\?\UNC\srv\share`, a path too long or with a reserved name)
/// is a spelling `cmd` refuses as a working folder and that tool paths would not match.
fn keep_canonical(canonical: &Path, stored: &Path) -> PathBuf {
    if canonical
        .as_os_str()
        .as_encoded_bytes()
        .starts_with(br"\\?\")
    {
        stored.to_path_buf()
    } else {
        canonical.to_path_buf()
    }
}

/// What an agent's tasks post to the actor, in the order it happens.
#[derive(Debug)]
pub(crate) enum AgentInput {
    /// An event from the agent's connection. Boxed: an `AcpEvent` is over 300 bytes, and
    /// moving the box through the actor's channel measured faster than moving the event.
    Event(Box<AcpEvent>),
    /// The agent is gone; nothing more comes from this agent.
    Ended(Ended),
}

/// How an agent ended.
#[derive(Debug, Clone)]
pub(crate) enum Ended {
    /// It exited on its own (crash, exit, or its stdout closed and it went).
    Exited(DetachReason),
    /// The host stopped it with the shutdown ladder. `report` is the exit report as a detach
    /// reason, for logs; the session itself detaches with `DetachReason::Closed`.
    Stopped {
        step: StopStep,
        report: DetachReason,
    },
}

/// The ladder step that ended a stopped agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopStep {
    /// It exited within 2 s of its stdin closing.
    StdinClosed,
    /// It exited within 3 s of SIGTERM (Unix only).
    Terminated,
    /// The kill ended it.
    Killed,
    /// It did not exit within 2 s of the kill; its tree was sent the kill and is reaped later.
    Unresponsive,
}

impl StopStep {
    fn of(outcome: &ShutdownOutcome) -> Self {
        match outcome {
            ShutdownOutcome::StdinClosed(_) => Self::StdinClosed,
            // The variant exists on every OS; only Unix ever returns it.
            ShutdownOutcome::Terminated(_) => Self::Terminated,
            ShutdownOutcome::Killed(_) => Self::Killed,
            ShutdownOutcome::Unresponsive(_) => Self::Unresponsive,
        }
    }
}

/// A running agent as the actor holds it. Dropping it stops the agent like [`AgentLink::stop`].
#[derive(Debug)]
pub(crate) struct AgentLink {
    /// The ACP connection; its calls run as spawned tasks that post their results.
    pub(crate) client: AcpClient,
    pid: u32,
    /// The agent's descendants, which a cancelled turn's sweep lists and ends
    /// ([`leftovers`]); it keeps nothing alive.
    descendants: Descendants,
    stop: Option<oneshot::Sender<()>>,
    /// The connection's event channel, read only to see whether it is empty ([`Self::fence`]).
    events: async_channel::Receiver<AcpEvent>,
    /// The fenced calls in flight, which the owner waits for before it posts [`Ended`].
    calls: Rc<Calls>,
}

impl AgentLink {
    /// The agent's process id, for logs.
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// The agent's descendants, for a cancelled turn's sweep.
    pub(crate) fn descendants(&self) -> &Descendants {
        &self.descendants
    }

    /// A future that ends once every event the connection has emitted so far was posted. A
    /// call's task awaits it before posting the call's result, so the result reaches the actor
    /// after every event the agent sent before its answer: the forwarder takes an event and posts
    /// it in one step on this thread, so an empty channel means all of them were posted.
    ///
    /// The call counts as in flight from this call until the fence ends or is dropped, and the
    /// owner posts [`Ended`] only once no call is in flight (within [`PUMP_END_BOUND`]), so the
    /// task must post its result in the same poll the fence ends in, as a spawned call does.
    pub(crate) fn fence(&self) -> impl Future<Output = ()> + 'static {
        let events = self.events.clone();
        let call = InFlight::new(&self.calls);
        async move {
            while !events.is_empty() {
                YieldNow(false).await;
            }
            drop(call);
        }
    }

    /// Whether the connection is gone: the event channel's only sender, held by R10's
    /// connection state, is dropped once the driver has ended (the agent's stdout ended, or
    /// a fatal error closed it). Every call then comes back `Closed`.
    pub(crate) fn closed(&self) -> bool {
        self.events.is_closed()
    }

    /// Asks the owner task to stop the agent with the shutdown ladder. It posts
    /// [`Ended::Stopped`] when done, or [`Ended::Exited`] if the agent exited first. A second
    /// call does nothing.
    pub(crate) fn stop(&mut self) {
        if let Some(stop) = self.stop.take()
            && stop.send(()).is_err()
        {
            tracing::debug!(
                pid = self.pid,
                "the agent had already ended when asked to stop"
            );
        }
    }
}

/// Launches the agent, connects ACP over its pipes and spawns its tasks on `spawner`. Every event
/// and the final [`Ended`] go to `post`. Nothing is sent to the agent until the actor calls the
/// client (`initialize` first).
pub(crate) fn start(
    spawner: &LocalSpawner,
    launcher: &dyn Launcher,
    spec: &LaunchSpec,
    clock: Arc<dyn Clock>,
    post: Rc<dyn Fn(AgentInput)>,
) -> Result<AgentLink, Error> {
    let Launched {
        stdin,
        stdout,
        stderr,
        handle,
        ..
    } = launcher.launch(spec)?;
    let pid = handle.pid();
    let descendants = handle.descendants();
    #[cfg(not(test))]
    let (client, events, driver) =
        clauseo_acp::connect(stdin, stdout, clauseo_acp::ConnectOptions::default());
    // Tests can hold back the connection task's wakes (`hold.rs`); with no hold it is the same.
    #[cfg(test)]
    let (client, events, driver) = hold::connect(stdin, stdout);
    let output = Output {
        progress: client.read_progress(),
        events: events.clone(),
    };
    // Each sender is dropped when its task ends, which wakes its receiver.
    let (pump_done, pump_end) = oneshot::channel::<()>();
    let (events_done, events_end) = oneshot::channel::<()>();
    let (stop, stop_asked) = oneshot::channel::<()>();

    spawn(spawner, async move {
        if let Err(err) = stderr.drain().await {
            tracing::debug!(pid, error = %err, "the agent's stderr pipe failed");
        }
        drop(pump_done);
    })?;
    spawn(spawner, async move {
        match driver.await {
            Ok(()) => tracing::debug!(pid, "the agent connection closed"),
            Err(err) => tracing::debug!(pid, error = %err, "the agent connection ended"),
        }
    })?;
    let forward = Rc::clone(&post);
    let fence_events = events.clone();
    spawn(spawner, async move {
        while let Ok(event) = events.recv().await {
            forward(AgentInput::Event(Box::new(event)));
        }
        drop(events_done);
    })?;
    let calls = Rc::new(Calls::default());
    let owner_calls = Rc::clone(&calls);
    let ends = async move {
        // An `Err(Canceled)` is the end itself: the task dropped its sender.
        let (_pump, _events) = join(pump_end, events_end).await;
        // The connection is gone, so each call in flight has its answer or `Closed` by now.
        owner_calls.idle().await;
    };
    spawn(spawner, own(handle, stop_asked, ends, output, clock, post))?;
    Ok(AgentLink {
        client,
        pid,
        descendants,
        stop: Some(stop),
        events: fence_events,
        calls,
    })
}

/// What the owner reads to tell whether all the agent wrote before a mark has been posted.
struct Output {
    progress: ReadProgress,
    /// The connection's event channel, read only to see whether it is empty or closed.
    events: async_channel::Receiver<AcpEvent>,
}

impl Output {
    /// Whether every event from what the agent wrote to stdout before `mark` has been posted:
    /// the channel is empty, and either nothing more can come (the connection is gone) or the
    /// reader has read it all and has nothing between it and the channel. The forwarder takes
    /// an event and posts it in one step on this thread, as [`AgentLink::fence`] relies on.
    fn posted_since(&self, mark: clauseo_acp::ReadMark) -> bool {
        self.events.is_empty() && (self.events.is_closed() || self.progress.drained_since(mark))
    }
}

/// The count of fenced calls in flight, and the owner's waker while it waits for none.
#[derive(Debug, Default)]
struct Calls {
    open: Cell<usize>,
    waiting: RefCell<Option<Waker>>,
}

impl Calls {
    /// Ends once no call is in flight.
    fn idle(&self) -> impl Future<Output = ()> + '_ {
        futures::future::poll_fn(move |cx| {
            if self.open.get() == 0 {
                return Poll::Ready(());
            }
            *self.waiting.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        })
    }

    /// Whether the owner is waiting for a call (the pipes have ended).
    #[cfg(test)]
    fn owner_waits(&self) -> bool {
        self.open.get() > 0 && self.waiting.borrow().is_some()
    }
}

/// One fenced call in flight; dropping it wakes the owner when it was the last.
struct InFlight(Rc<Calls>);

impl InFlight {
    fn new(calls: &Rc<Calls>) -> Self {
        calls.open.set(calls.open.get() + 1);
        Self(Rc::clone(calls))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let open = self.0.open.get().saturating_sub(1);
        self.0.open.set(open);
        if open == 0
            && let Some(waker) = self.0.waiting.borrow_mut().take()
        {
            waker.wake();
        }
    }
}

/// Gives the pool's other tasks one turn before going on.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

fn spawn(spawner: &LocalSpawner, task: impl Future<Output = ()> + 'static) -> Result<(), Error> {
    spawner
        .spawn_local(task)
        .map_err(|_shut_down| Error::Closed)
}

/// The owner task: exit or stop, then the bounded wait for the pipes, then the report.
async fn own(
    mut handle: Handle,
    stop_asked: oneshot::Receiver<()>,
    ends: impl Future<Output = ()>,
    output: Output,
    clock: Arc<dyn Clock>,
    post: Rc<dyn Fn(AgentInput)>,
) {
    let pid = handle.pid();
    let exited = {
        let exit = pin!(handle.exited());
        // A dropped `AgentLink` (Canceled) asks for a stop too.
        match select(exit, stop_asked).await {
            Either::Left((report, _)) => Some(report),
            Either::Right(_) => None,
        }
    };
    let ended = if let Some(report) = exited {
        // The agent is gone: all it wrote is in its stdout or already read.
        let mark = output.progress.mark();
        settle(ends, || output.posted_since(mark), &*clock).await;
        Ended::Exited(report.detach_reason())
    } else {
        // Listed off the actor first (Unix only: Windows ends the whole job): the tree kill
        // ends a descendant that left the agent's group, as a Codex command does.
        let listed = if cfg!(unix) {
            leftovers::snapshot(handle.descendants(), Arc::clone(&clock)).await
        } else {
            None
        };
        let outcome = handle
            .shutdown(&ProcessTimer(Arc::clone(&clock)), listed)
            .await;
        let step = StopStep::of(&outcome);
        if step == StopStep::Unresponsive {
            tracing::warn!(
                pid,
                "the agent did not exit after its kill; it is reaped later"
            );
        }
        let mark = output.progress.mark();
        settle(ends, || output.posted_since(mark), &*clock).await;
        Ended::Stopped {
            step,
            report: outcome.report().detach_reason(),
        }
    };
    post(AgentInput::Ended(ended));
}

/// Waits for `ends` on the host clock; true when it ended. Each time [`PUMP_END_BOUND`] passes
/// it gives up if `posted` holds (what the agent wrote before it ended is all posted), and
/// otherwise waits one bound more, up to [`PUMP_END_ROUNDS`] bounds.
async fn settle(
    ends: impl Future<Output = ()>,
    posted: impl Fn() -> bool,
    clock: &dyn Clock,
) -> bool {
    let mut ends = pin!(ends);
    for _ in 0..PUMP_END_ROUNDS {
        match select(ends.as_mut(), clock.after(PUMP_END_BOUND)).await {
            Either::Left(_) => return true,
            Either::Right(_) if posted() => {
                tracing::warn!(
                    "the agent's pipes stayed open after it ended; reporting without them"
                );
                return false;
            }
            Either::Right(_) => {}
        }
    }
    tracing::warn!("the agent's stdout was still being read long after it ended; reporting now");
    false
}

#[cfg(test)]
pub(crate) mod hold;
#[cfg(test)]
mod tests;
