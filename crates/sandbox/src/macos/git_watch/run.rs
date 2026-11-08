//! The git watch's driver: one thread per session that feeds the [`Watch`] the stream's
//! events, checks the stream with a canary, scans every 2 s when the stream could not start,
//! runs the 30 s pass over changed folders, ends every process of the session on the first
//! find, and once none is left runs the final scan, which sets aside.
//!
//! The canary: every 10 s the thread rewrites a file in the set-aside folder it uses, which is
//! on the matter's volume, out of the agent's reach, and watched by the same stream. When no
//! event for it comes within 2 s, or it can't be written, the stream is lost: the session ends
//! as for a find (kill first, then the final scan) and the lawyer gets
//! [`Denial::GitWatchLost`](crate::Denial). An event that says events were dropped or must be
//! rescanned only rescans the matter at once, and the watch carries on.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::aside::{Aside, Stuck};
use super::host::{self, LIVE};
use super::state::{self, FileStore, SessionRecord, Store};
use super::{Asks, Event, Fs, Launch, Phase, Watch, fsevents, os};
use crate::macos::jail_procs::{self, Probe};
use crate::{Clock, DenialSink, Policy, SystemClock};

/// How often the thread looks at the session when no event comes.
const TICK: Duration = Duration::from_millis(100);

/// How often the matter is scanned whole when the stream could not start.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(2);

/// How often the session's kill is sent again while a process of it is left.
const KILL_AGAIN: Duration = Duration::from_millis(100);

/// How long the watch waits for a killed session to go before the final scan runs anyway.
const KILL_CAP: Duration = Duration::from_secs(10);

/// How often the canary file is rewritten.
pub(crate) const CANARY_EVERY: Duration = Duration::from_secs(10);

/// How long the canary's event may take before the stream counts as lost.
pub(crate) const CANARY_WAIT: Duration = Duration::from_secs(2);

/// The canary file's name in the set-aside folder.
const CANARY: &str = "watch-canary";

/// How often the pass over changed folders runs.
pub(crate) const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// The most time one pass over changed folders takes; the next goes on where it stopped.
pub(crate) const SWEEP_CAP: Duration = Duration::from_millis(250);

/// The jailed session as the watch sees it: whether every process of it is gone, and its kill.
pub(crate) trait Session: Send + Sync {
    /// Whether every process of the session has exited: its group, its tree and every process
    /// carrying its probe.
    fn gone(&self) -> bool;
    /// Sends every process of the session SIGKILL.
    fn end(&self);
}

/// The session a thread's guard ends if the thread dies.
type Slot = Arc<Mutex<Option<Arc<dyn Session>>>>;

/// What reaches the watch's thread.
pub(crate) enum Msg {
    /// A batch from the stream.
    Events(Vec<Event>),
    /// The agent started: its session.
    Attach(Arc<dyn Session>),
    /// The agent did not start.
    Abandon,
}

/// The canary file and its state.
struct Canary {
    path: PathBuf,
    next_touch: Duration,
    /// When the oldest write whose event hasn't come went.
    sent: Option<Duration>,
}

/// The decisions of one session's watch, driven by [`Runner::input`]: no thread, and only the
/// 30 s pass's cap reads a clock of its own, so the tests drive it step by step.
pub(crate) struct Runner<F> {
    watch: Watch<F>,
    /// The stream could not start: scan whole every [`RESCAN_EVERY`].
    no_stream: bool,
    /// When the canary's event did not come, or the canary could not be written: the session
    /// ends.
    lost: Option<Duration>,
    next_scan: Duration,
    canary: Option<Canary>,
    next_sweep: Duration,
    /// The clock the 30 s pass's cap reads.
    clock: Arc<dyn Clock>,
    /// The kill's cap ran out with a process of the session left.
    lingering: bool,
    session: Option<Arc<dyn Session>>,
    slot: Slot,
    abandoned: bool,
    /// When a find first asked for the session's end.
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
    pub(crate) fn new(watch: Watch<F>, no_stream: bool, now: Duration) -> Runner<F> {
        Runner {
            watch,
            no_stream,
            lost: None,
            next_scan: now,
            canary: None,
            next_sweep: now + SWEEP_EVERY,
            clock: Arc::new(SystemClock::new()),
            lingering: false,
            session: None,
            slot: Slot::default(),
            abandoned: false,
            ending: None,
            killed: None,
        }
    }

    /// The runner with the canary file `path`, first written at `now`; without a stream it is
    /// never written.
    pub(crate) fn with_canary(mut self, path: PathBuf, now: Duration) -> Runner<F> {
        if !self.no_stream {
            self.canary = Some(Canary {
                path,
                next_touch: now,
                sent: None,
            });
        }
        self
    }

    /// The runner with the 30 s pass's cap read from `clock`.
    #[cfg(test)]
    pub(crate) fn with_clock(mut self, clock: Arc<dyn Clock>) -> Runner<F> {
        self.clock = clock;
        self
    }

    /// Whether the stream was lost.
    #[cfg(test)]
    pub(crate) fn lost(&self) -> bool {
        self.lost.is_some()
    }

    /// Whether a process of the session may still run: the kill's cap ran out first.
    pub(crate) fn lingering(&self) -> bool {
        self.lingering
    }

    /// One input (`None` for a tick) at `now`.
    pub(crate) fn input(&mut self, msg: Option<Msg>, now: Duration) -> Flow {
        let mut asks = Asks::default();
        match msg {
            Some(Msg::Events(events)) => {
                let events = self.echoes(events);
                asks = self.watch.events(&events);
            }
            Some(Msg::Attach(session)) => {
                *self.slot.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Arc::clone(&session));
                self.session = Some(session);
            }
            Some(Msg::Abandon) => self.abandoned = true,
            None => {}
        }
        asks.end |= self.canary(now);
        if asks.rescan || (self.no_stream && now >= self.next_scan) {
            asks = asks.and(self.watch.full_scan(Phase::Running));
            self.next_scan = now + RESCAN_EVERY;
        }
        if self.ending.is_none() && now >= self.next_sweep {
            let clock = Arc::clone(&self.clock);
            let until = clock.now() + SWEEP_CAP;
            let (found, _) = self.watch.sweep(&|| clock.now() >= until);
            asks = asks.and(found);
            self.next_sweep = now + SWEEP_EVERY;
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
            tracing::error!(
                "the session's processes did not go after its kill; checking anyway, and keeping \
                 its record for the next start or launch"
            );
            self.lingering = true;
            Flow::Done
        } else {
            Flow::Go
        }
    }

    /// `events` without the canary's, which mark the stream as working, and without the rest
    /// of the set-aside folder's, which are not the matter's.
    fn echoes(&mut self, events: Vec<Event>) -> Vec<Event> {
        let Some(canary) = &mut self.canary else {
            return events;
        };
        let folder = canary.path.parent().unwrap_or(&canary.path);
        let matter = self.watch.matter();
        let mut heard = false;
        let events = events
            .into_iter()
            .filter(|event| {
                if event.path == canary.path {
                    heard = true;
                    return false;
                }
                !event.path.starts_with(folder) || event.path.starts_with(matter)
            })
            .collect();
        if heard {
            canary.sent = None;
        }
        events
    }

    /// Rewrites the canary when due. Returns true once, when the stream is lost: the canary's
    /// event is late, or the canary can't be written. The lawyer is told then.
    fn canary(&mut self, now: Duration) -> bool {
        let Some(canary) = &mut self.canary else {
            return false;
        };
        if self.lost.is_some() {
            return false;
        }
        let late = canary
            .sent
            .is_some_and(|at| now.saturating_sub(at) >= CANARY_WAIT);
        let mut lost = late;
        if !lost && now >= canary.next_touch {
            canary.next_touch = now + CANARY_EVERY;
            match self.watch.fs().touch(&canary.path) {
                Ok(()) => {
                    canary.sent.get_or_insert(now);
                }
                Err(error) => {
                    tracing::error!(%error, "the git watch's canary can't be written");
                    lost = true;
                }
            }
        }
        if lost {
            tracing::error!(
                matter = %self.watch.matter().display(),
                "the git watch lost its FSEvents stream; ending the session"
            );
            self.lost = Some(now);
            self.watch.lost_stream();
        }
        lost
    }

    /// The final scan, every process gone: each find is set aside. Returns what could neither
    /// move nor be neutralised.
    pub(crate) fn finish(mut self) -> Vec<Stuck> {
        let asks = self.watch.full_scan(Phase::Ended);
        debug_assert!(!asks.end, "the final scan never asks to end");
        self.watch.take_stuck()
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
    /// The agent runs: the watch now ends with its session. A watch whose thread is gone
    /// ends the session at once (fail-closed).
    pub(crate) fn attach(mut self, session: Arc<dyn Session>) {
        if let Some(tx) = self.tx.take()
            && let Err(mpsc::SendError(Msg::Attach(session))) = tx.send(Msg::Attach(session))
        {
            tracing::error!("the git watch's thread is gone; ending the session");
            session.end();
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

/// Starts the watch of `policy`'s matter for `session` (an id for the records) with `probe`,
/// before the agent: the set-aside folders are made, the matter's state is checked (refused
/// while an item an earlier scan could not handle is there; a dead session's final scan run),
/// the launch's items noted and the session recorded, and the stream started (or, failing
/// that, the 2 s rescan, logged). Refused without a set-aside folder or when one can't be
/// made: the agent never runs without the backstop.
pub(crate) fn start(
    policy: &Policy,
    session: &str,
    probe: &Probe,
    notices: Arc<dyn DenialSink>,
) -> std::io::Result<Watching> {
    let data = policy
        .git_set_aside()
        .ok_or_else(|| std::io::Error::other("the policy names no git set-aside folder"))?;
    os::Real.make_dirs(data)?;
    let folder = match policy.volume_set_aside() {
        Some(dir) => os::volume_folder(dir)?,
        None => data.to_owned(),
    };
    let matter = policy.matter().to_owned();
    let store = FileStore::new(data, &matter);
    let this = jail_procs::this_process();
    state::begin(&store, &os::Real, &matter, &notices, &host::host())?;
    let session = session.to_owned();
    let canary = os::Real
        .resolve(&folder)
        .unwrap_or_else(|| folder.clone())
        .join(CANARY);
    let aside = Aside::new(matter.clone(), folder.clone(), session.clone(), notices);
    let watch = Watch::new(os::Real, matter, aside, &Launch::of(policy));
    let kill = probe.clone();
    LIVE.add(
        &session,
        Arc::new(move || {
            kill.kill_all();
        }),
    )?;
    let record = SessionRecord {
        session: session.clone(),
        host_pid: this.0,
        host_start: this.1,
        probe: probe.name().to_owned(),
        set_aside: folder.as_path().into(),
        baseline: watch.baseline().clone(),
    };
    let started = state::record(&store, record)
        .and_then(|()| start_with(watch, session.clone(), store, Some(canary), fsevents::start));
    if started.is_err() {
        LIVE.remove(&session);
    }
    started
}

/// Ends the session if the thread dies (fail-closed), and takes it off the host's register
/// whatever way the thread ends.
struct Guard {
    slot: Slot,
    session: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        LIVE.remove(&self.session);
        if std::thread::panicking() {
            tracing::error!("the git watch's thread panicked; ending the session");
            if let Some(session) = &*self.slot.lock().unwrap_or_else(PoisonError::into_inner) {
                session.end();
            }
        }
    }
}

/// [`start`] with the watch made and the stream's source given: the tests pass a fake one and
/// a store in memory. The source watches the matter, and the canary's folder when it is
/// outside the matter.
pub(crate) fn start_with<F, S, T>(
    watch: Watch<F>,
    session: String,
    store: T,
    canary: Option<PathBuf>,
    source: impl FnOnce(&[PathBuf], Sender<Msg>) -> Result<S, String>,
) -> std::io::Result<Watching>
where
    F: Fs + 'static,
    S: Send + 'static,
    T: Store,
{
    let (tx, rx) = mpsc::channel();
    let matter = watch.matter().to_owned();
    let mut roots = vec![matter.clone()];
    roots.extend(
        canary
            .as_deref()
            .and_then(Path::parent)
            .filter(|folder| !folder.starts_with(&matter))
            .map(Path::to_owned),
    );
    let stream = match source(&roots, tx.clone()) {
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
            let now = clock.now();
            let mut runner = Runner::new(watch, stream.is_none(), now);
            if let Some(canary) = canary {
                runner = runner.with_canary(canary, now);
            }
            let _guard = Guard {
                slot: Arc::clone(&runner.slot),
                session: session.clone(),
            };
            drive(&mut runner, &rx, &clock);
            drop(stream);
            let keep = runner.lingering();
            let stuck = runner.finish();
            state::end(&store, &session, stuck, keep);
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
