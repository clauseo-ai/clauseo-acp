//! The git watch's driver: one thread per session that feeds the [`Watch`] the stream's
//! events, scans every 2 s when the stream could not start, ends every process of the session
//! on the first find, and once none is left runs the final scan, which sets aside.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::aside::{Aside, Stuck};
use super::state::{self, FileStore, Host, SessionRecord, Store};
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

/// How long a launch waits for a dead session's processes to go.
const DEAD_KILL_WAIT: Duration = Duration::from_secs(3);

/// The sessions whose watch runs in this process, by name: a recorded session of this process
/// that is not here never ran its final scan.
static LIVE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn live(change: impl FnOnce(&mut HashSet<String>) -> bool) -> bool {
    let mut guard = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    change(guard.get_or_insert_with(HashSet::new))
}

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

/// The decisions of one session's watch, driven by [`Runner::input`]: no thread, no clock of
/// its own, so the tests drive it step by step.
pub(crate) struct Runner<F> {
    watch: Watch<F>,
    /// The stream could not start: scan whole every [`RESCAN_EVERY`].
    degraded: bool,
    next_scan: Duration,
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
    pub(crate) fn new(watch: Watch<F>, degraded: bool, now: Duration) -> Runner<F> {
        Runner {
            watch,
            degraded,
            next_scan: now,
            session: None,
            slot: Slot::default(),
            abandoned: false,
            ending: None,
            killed: None,
        }
    }

    /// One input (`None` for a tick) at `now`.
    pub(crate) fn input(&mut self, msg: Option<Msg>, now: Duration) -> Flow {
        let mut asks = Asks::default();
        match msg {
            Some(Msg::Events(events)) => asks = self.watch.events(&events),
            Some(Msg::Attach(session)) => {
                *self.slot.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Arc::clone(&session));
                self.session = Some(session);
            }
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
            tracing::error!("the session's processes did not go after its kill; checking anyway");
            Flow::Done
        } else {
            Flow::Go
        }
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
    let dead = |record: &SessionRecord| {
        if (record.host_pid, record.host_start) == this {
            !live(|set| set.contains(&record.session))
        } else {
            jail_procs::start_of(record.host_pid).is_none_or(|s| Some(s) != record.host_start)
        }
    };
    let kill = |name: &str| kill_dead(name);
    let host = Host {
        dead: &dead,
        kill: &kill,
    };
    state::begin(&store, &os::Real, &matter, &notices, &host)?;
    let session = session.to_owned();
    let aside = Aside::new(matter.clone(), folder.clone(), session.clone(), notices);
    let watch = Watch::new(os::Real, matter, aside, &Launch::of(policy));
    live(|set| set.insert(session.clone()));
    let record = SessionRecord {
        session: session.clone(),
        host_pid: this.0,
        host_start: this.1,
        probe: probe.name().to_owned(),
        set_aside: folder.as_path().into(),
        baseline: watch.baseline().clone(),
    };
    let started = state::record(&store, record)
        .and_then(|()| start_with(watch, session.clone(), store, fsevents::start));
    if started.is_err() {
        live(|set| set.remove(&session));
    }
    started
}

/// Ends every process a dead session left, by its probe name, waiting a little for them.
fn kill_dead(name: &str) {
    let Some(probe) = Probe::from_name(name) else {
        tracing::error!(name, "a recorded probe name Clauseo never makes");
        return;
    };
    let clock = SystemClock::new();
    let until = clock.now() + DEAD_KILL_WAIT;
    while !probe.gone() && clock.now() < until {
        probe.kill_all();
        async_io::block_on(async_io::Timer::after(Duration::from_millis(50)));
    }
    if !probe.gone() {
        tracing::error!(
            name,
            "a dead session's processes did not go; scanning anyway"
        );
    }
}

/// Ends the session if the thread dies (fail-closed), and takes it off [`LIVE`] whatever way
/// the thread ends.
struct Guard {
    slot: Slot,
    session: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        live(|set| set.remove(&self.session));
        if std::thread::panicking() {
            tracing::error!("the git watch's thread panicked; ending the session");
            if let Some(session) = &*self.slot.lock().unwrap_or_else(PoisonError::into_inner) {
                session.end();
            }
        }
    }
}

/// [`start`] with the watch made and the stream's source given: the tests pass a fake one and
/// a store in memory.
pub(crate) fn start_with<F, S, T>(
    watch: Watch<F>,
    session: String,
    store: T,
    source: impl FnOnce(&Path, Sender<Msg>) -> Result<S, String>,
) -> std::io::Result<Watching>
where
    F: Fs + 'static,
    S: Send + 'static,
    T: Store,
{
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
            let _guard = Guard {
                slot: Arc::clone(&runner.slot),
                session: session.clone(),
            };
            drive(&mut runner, &rx, &clock);
            drop(stream);
            let stuck = runner.finish();
            state::end(&store, &session, stuck);
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
