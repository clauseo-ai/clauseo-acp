//! The git watch's driver: one thread per session that feeds the [`Watch`] the stream's
//! events, scans every 2 s when the stream could not start, ends the session when a move
//! fails, and runs the final scan once the jailed tree is gone.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use super::{Asks, Event, Fs, Launch, Phase, Watch, fsevents, os};
use crate::{Clock, DenialSink, Policy, SystemClock};

/// How often the thread looks at the session when no event comes.
const TICK: Duration = Duration::from_millis(100);

/// How often the matter is scanned whole when the stream could not start.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(2);

/// How often the session's kill is sent again while its tree is not gone.
const KILL_AGAIN: Duration = Duration::from_secs(1);

/// How long the watch waits for a killed tree to go before the final scan runs anyway.
const KILL_CAP: Duration = Duration::from_secs(10);

/// The jailed session as the watch sees it: whether its tree is gone, and its kill.
pub(crate) trait Session: Send {
    /// Whether the jailed process tree has exited.
    fn gone(&self) -> bool;
    /// Ends the jailed process tree, through the process crate's kill.
    fn end(&self);
}

/// What reaches the watch's thread.
pub(crate) enum Msg {
    /// A batch from the stream.
    Events(Vec<Event>),
    /// The agent started: its session.
    Attach(Box<dyn Session>),
    /// The agent did not start.
    Abandon,
}

/// The decisions of one session's watch, driven by [`Runner::input`]: no thread, no clock of
/// its own, so the tests drive it step by step.
pub(crate) struct Runner<F> {
    watch: Watch<F>,
    /// The stream could not start: scan whole every [`RESCAN_EVERY`].
    degraded: bool,
    next_scan: Duration,
    session: Option<Box<dyn Session>>,
    abandoned: bool,
    /// When a failed move first asked for the session's end.
    ending: Option<Duration>,
    /// When the session's kill last went.
    killed: Option<Duration>,
}

/// Whether the runner goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    Go,
    /// The session is over: run [`Runner::finish`].
    Done,
}

impl<F: Fs> Runner<F> {
    pub(crate) fn new(watch: Watch<F>, degraded: bool, now: Duration) -> Runner<F> {
        Runner {
            watch,
            degraded,
            next_scan: now,
            session: None,
            abandoned: false,
            ending: None,
            killed: None,
        }
    }

    /// One input (`None` for a tick) at `now`.
    pub(crate) fn input(&mut self, msg: Option<Msg>, now: Duration) -> Flow {
        let mut asks = Asks::default();
        match msg {
            Some(Msg::Events(events)) => asks = self.watch.events(&events, Phase::Running),
            Some(Msg::Attach(session)) => self.session = Some(session),
            Some(Msg::Abandon) => self.abandoned = true,
            None => {}
        }
        if asks.rescan || (self.degraded && now >= self.next_scan) {
            asks = asks.and(self.watch.full_scan(Phase::Running));
            self.next_scan = now + RESCAN_EVERY;
        }
        if asks.end && self.ending.is_none() {
            self.watch.ended_session();
            self.ending = Some(now);
        }
        if self.abandoned {
            return Flow::Done;
        }
        let Some(session) = &self.session else {
            return Flow::Go;
        };
        let Some(since) = self.ending else {
            return if session.gone() { Flow::Done } else { Flow::Go };
        };
        if !session.gone()
            && self
                .killed
                .is_none_or(|at| now.saturating_sub(at) >= KILL_AGAIN)
        {
            session.end();
            self.killed = Some(now);
        }
        if session.gone() {
            Flow::Done
        } else if now.saturating_sub(since) >= KILL_CAP {
            tracing::error!("the session's tree did not go after its kill; checking anyway");
            Flow::Done
        } else {
            Flow::Go
        }
    }

    /// The final scan, the agent gone: what still can't move is renamed in place.
    pub(crate) fn finish(mut self) {
        let asks = self.watch.full_scan(Phase::Ended);
        debug_assert!(!asks.end, "the final scan never asks to end");
    }
}

/// A session's watch, started before its agent. [`Watching::attach`] hands it the agent once
/// it runs; dropped without that, it says the agent did not start, and the watch scans once
/// and stops.
#[derive(Debug)]
pub(crate) struct Watching {
    tx: Option<Sender<Msg>>,
}

impl Watching {
    /// The agent runs: the watch now ends with its tree.
    pub(crate) fn attach(mut self, session: Box<dyn Session>) {
        if let Some(tx) = self.tx.take()
            && tx.send(Msg::Attach(session)).is_err()
        {
            tracing::error!("the git watch's thread is gone");
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take()
            && tx.send(Msg::Abandon).is_err()
        {
            tracing::error!("the git watch's thread is gone");
        }
    }
}

/// Starts the watch of `policy`'s matter for `session` (an id for the records), before the
/// agent: the set-aside folder is made, the launch's `.git`s noted, and the stream started (or,
/// failing that, the 2 s rescan, logged). Refused when the policy names no set-aside folder or
/// it can't be made: the agent never runs without the backstop.
pub(crate) fn start(
    policy: &Policy,
    session: String,
    notices: Arc<dyn DenialSink>,
) -> std::io::Result<Watching> {
    let set_aside = policy
        .git_set_aside()
        .ok_or_else(|| std::io::Error::other("the policy names no git set-aside folder"))?;
    let watch = Watch::new(
        os::Real,
        policy.matter().to_owned(),
        set_aside.to_owned(),
        session,
        &Launch::of(policy),
        notices,
    );
    start_with(watch, fsevents::start)
}

/// [`start`] with the watch made and the stream's source given: the tests pass a fake one.
pub(crate) fn start_with<F, S>(
    watch: Watch<F>,
    source: impl FnOnce(&Path, Sender<Msg>) -> Result<S, String>,
) -> std::io::Result<Watching>
where
    F: Fs + 'static,
    S: Send + 'static,
{
    watch.make_set_aside()?;
    let (tx, rx) = mpsc::channel();
    let matter = watch.matter().to_owned();
    let stream = match source(&matter, tx.clone()) {
        Ok(stream) => Some(stream),
        Err(error) => {
            tracing::error!(
                matter = %matter.display(), %error,
                "the git watch's FSEvents stream did not start; scanning the matter every 2 s"
            );
            None
        }
    };
    // The thread holds a sender of its own: without a stream, the launch's is the only other
    // one, and `Watching::attach` drops it. The end comes from the session, or an abandon.
    let own = tx.clone();
    std::thread::Builder::new()
        .name("clauseo-git-watch".to_owned())
        .spawn(move || {
            let _own = own;
            let clock = SystemClock::new();
            let mut runner = Runner::new(watch, stream.is_none(), clock.now());
            drive(&mut runner, &rx, &clock);
            drop(stream);
            runner.finish();
            tracing::debug!(matter = %matter.display(), "the git watch stopped");
        })?;
    Ok(Watching { tx: Some(tx) })
}

/// Feeds `runner` until it is done. Every sender gone counts as an abandon.
fn drive<F: Fs>(runner: &mut Runner<F>, rx: &Receiver<Msg>, clock: &dyn Clock) {
    loop {
        let msg = match rx.recv_timeout(TICK) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Msg::Abandon),
        };
        if runner.input(msg, clock.now()) == Flow::Done {
            return;
        }
    }
}
