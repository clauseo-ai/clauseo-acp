//! One agent process and its ACP connection, driven by four tasks on the actor's `LocalPool`:
//! R10's `Driver`, the event forwarder, S7's stderr pump, and the owner task that holds S7's
//! `Handle`. The owner waits for the agent to exit or for a stop; on a stop it drops the `exited`
//! future and runs the shutdown ladder, because `exited` borrows the handle and `shutdown`
//! consumes it. Either way it then waits, at most [`PUMP_END_BOUND`], for the stderr pump and the
//! event forwarder to end, reads the exit report, and posts [`Ended`] after the agent's last event.

mod spec;

use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use clauseo_acp::{AcpClient, AcpEvent, ConnectOptions};
use clauseo_process::{Handle, LaunchSpec, Launched, Launcher, ShutdownOutcome};
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
/// agent left holding a pipe cannot hold the report back longer than this.
pub(crate) const PUMP_END_BOUND: Duration = Duration::from_secs(1);

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
            #[cfg(unix)]
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
    stop: Option<oneshot::Sender<()>>,
    /// The connection's event channel, read only to see whether it is empty ([`Self::fence`]).
    events: async_channel::Receiver<AcpEvent>,
}

impl AgentLink {
    /// The agent's process id, for logs.
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// A future that ends once every event the connection has emitted so far was posted. A
    /// call's task awaits it before posting the call's result, so the result reaches the actor
    /// after every event the agent sent before its answer: the forwarder takes an event and posts
    /// it in one step on this thread, so an empty channel means all of them were posted.
    pub(crate) fn fence(&self) -> impl Future<Output = ()> + 'static {
        let events = self.events.clone();
        async move {
            while !events.is_empty() {
                YieldNow(false).await;
            }
        }
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
    let (client, events, driver) = clauseo_acp::connect(stdin, stdout, ConnectOptions::default());
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
    let ends = async move {
        // An `Err(Canceled)` is the end itself: the task dropped its sender.
        let (_pump, _events) = join(pump_end, events_end).await;
    };
    spawn(spawner, own(handle, stop_asked, ends, clock, post))?;
    Ok(AgentLink {
        client,
        pid,
        stop: Some(stop),
        events: fence_events,
    })
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
        settle(ends, &*clock).await;
        Ended::Exited(report.detach_reason())
    } else {
        let outcome = handle.shutdown(&ProcessTimer(Arc::clone(&clock))).await;
        let step = StopStep::of(&outcome);
        if step == StopStep::Unresponsive {
            tracing::warn!(
                pid,
                "the agent did not exit after its kill; it is reaped later"
            );
        }
        settle(ends, &*clock).await;
        Ended::Stopped {
            step,
            report: outcome.report().detach_reason(),
        }
    };
    post(AgentInput::Ended(ended));
}

/// Waits for `ends` at most [`PUMP_END_BOUND`] on the host clock; true when it ended in time.
async fn settle(ends: impl Future<Output = ()>, clock: &dyn Clock) -> bool {
    match select(pin!(ends), clock.after(PUMP_END_BOUND)).await {
        Either::Left(_) => true,
        Either::Right(_) => {
            tracing::warn!("the agent's pipes stayed open after it ended; reporting without them");
            false
        }
    }
}

#[cfg(test)]
mod tests;
