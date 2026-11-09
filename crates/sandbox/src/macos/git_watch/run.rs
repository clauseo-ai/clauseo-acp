//! The git watch's driver: one thread per watched folder of a session (the matter, and the
//! session's notes folder) that feeds the [`Watch`] the stream's events, checks the stream with
//! a canary, runs the 30 s pass over changed folders, ends every process of the session on the
//! first find, and once none is left runs the final scan, which sets aside.
//!
//! A launch whose stream can't start is refused: the agent never runs on a weaker watch. The
//! canary: every 10 s the thread rewrites a file in the set-aside folder it uses, which is on
//! the watched folder's volume, out of the agent's reach, and watched by the same stream. When
//! no event for it comes within 2 s, or it can't be written, the stream is lost: the session
//! ends as for a find (kill first, then the final scan) and the lawyer gets
//! [`Denial::GitWatchLost`](crate::Denial). Each batch is stamped when the stream queues it,
//! and one read 2 s or more after that ends the session the same way: the watch lags behind
//! the agent (one that writes nonstop would otherwise hold the canary's event back behind its
//! own for as long as it writes). An event that says events were dropped or must be
//! rescanned only rescans the folder at once, and the watch carries on. A check that runs past
//! the watchdog's budget without progress (`run/watchdog.rs`) ends the session the same way.
//!
//! The kill comes first: the first find of an input returns at once, before any rescan or
//! 30 s pass, and once the session is ending nothing more is checked until it has gone.
//! Nothing moves while a process of the session is left: when one outlives the kill's 10 s,
//! the final scan only lists what it would set aside, and the session's record stays marked
//! for the matter's next launch, which is refused until a kill finds none left. The same holds
//! for an agent that did not start when a process carrying the launch's probe is left.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::aside::Stuck;
use super::{Event, Fs, Phase, Watch};
use crate::{Clock, SystemClock};

mod start;
mod watchdog;

pub(crate) use start::start;
#[cfg(test)]
pub(crate) use start::start_with;
#[cfg(test)]
pub(crate) use watchdog::{BUDGET, Busy};

/// How often the thread looks at the session when no event comes.
const TICK: Duration = Duration::from_millis(100);

/// How often the session's kill is sent again while a process of it is left.
const KILL_AGAIN: Duration = Duration::from_millis(100);

/// How long the watch waits for a killed session to go before it gives up: nothing moves, and
/// the session's record stays for the next start or launch.
pub(crate) const KILL_CAP: Duration = Duration::from_secs(10);

/// How often the canary file is rewritten.
pub(crate) const CANARY_EVERY: Duration = Duration::from_secs(10);

/// How long the canary's event may take before the stream counts as lost.
pub(crate) const CANARY_WAIT: Duration = Duration::from_secs(2);

/// The canary file's name in the set-aside folder.
const CANARY: &str = "watch-canary";

/// The notes folder's canary file's name in the set-aside folder.
const CANARY_NOTES: &str = "watch-canary-notes";

/// How many times a canary write is tried before the stream counts as lost: a write can fail
/// once for a moment (a create racing another process's in the same folder).
const CANARY_TRIES: u32 = 3;

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

/// How a session's folder is watched while the agent runs; its session record keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    /// The stream, its canary and the 30 s pass, with the scans before and after.
    #[default]
    Live,
    /// No stream: only the launch's state check and walk before, and the final scan after.
    AfterOnly,
}

/// The clock the watch's threads read and the stream stamps each batch with
/// ([`Msg::queued`]).
pub(crate) fn queue_clock() -> &'static SystemClock {
    static CLOCK: OnceLock<SystemClock> = OnceLock::new();
    CLOCK.get_or_init(SystemClock::new)
}

/// What reaches the watch's thread.
pub(crate) enum Msg {
    /// A batch from the stream, and when the stream queued it on [`queue_clock`].
    Events(Vec<Event>, Duration),
    /// The agent started: its session.
    Attach(Arc<dyn Session>),
    /// The agent did not start.
    Abandon,
}

impl Msg {
    /// `events`, stamped now.
    pub(crate) fn queued(events: Vec<Event>) -> Msg {
        Msg::Events(events, queue_clock().now())
    }
}

/// The canary file and its state.
struct Canary {
    path: PathBuf,
    next_touch: Duration,
    /// When the oldest write whose event hasn't come went.
    sent: Option<Duration>,
}

/// The decisions of one session's watch, driven by [`Runner::input`]: no thread, and only the
/// 30 s pass's cap and the watchdog's [`watchdog::Busy`] read a clock of their own, so the
/// tests drive it step by step.
pub(crate) struct Runner<F> {
    watch: Watch<F>,
    mode: Mode,
    /// When the canary's event did not come, or the canary could not be written: the session
    /// ends.
    lost: Option<Duration>,
    canary: Option<Canary>,
    next_sweep: Duration,
    /// The clock the 30 s pass's cap reads.
    clock: Arc<dyn Clock>,
    /// When each check begins and ends, for the watchdog, with the session.
    busy: Arc<watchdog::Busy>,
    /// The kill's cap ran out with a process of the session left.
    lingering: bool,
    session: Option<Arc<dyn Session>>,
    /// The launch's probe, as a session: what an agent that did not start may have left.
    probe: Option<Arc<dyn Session>>,
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

/// How the final scan went.
#[must_use]
pub(crate) enum End {
    /// Every process gone, each find set aside: what could neither move nor be neutralised.
    Scanned(Vec<Stuck>),
    /// A process outlived the kill: nothing moved, and these are what the scan found.
    Outlived(Vec<PathBuf>),
}

impl End {
    /// What could neither move nor be neutralised; panics when the session outlived the kill.
    #[cfg(test)]
    pub(crate) fn stuck(self) -> Vec<Stuck> {
        match self {
            End::Scanned(stuck) => stuck,
            End::Outlived(finds) => panic!("the session outlived its kill: {finds:?}"),
        }
    }
}

impl<F: Fs> Runner<F> {
    pub(crate) fn new(watch: Watch<F>, mode: Mode, now: Duration) -> Runner<F> {
        Runner {
            watch,
            mode,
            lost: None,
            canary: None,
            next_sweep: now + SWEEP_EVERY,
            clock: Arc::new(SystemClock::new()),
            busy: watchdog::Busy::new(watchdog::clock()),
            lingering: false,
            session: None,
            probe: None,
            abandoned: false,
            ending: None,
            killed: None,
        }
    }

    /// The runner with the canary file `path`, first written at `now`; without a stream it is
    /// never written.
    pub(crate) fn with_canary(mut self, path: PathBuf, now: Duration) -> Runner<F> {
        if self.mode == Mode::Live {
            self.canary = Some(Canary {
                path,
                next_touch: now,
                sent: None,
            });
        }
        self
    }

    /// The runner with the launch's probe: after an abandon, nothing moves while a process
    /// carrying it is left.
    pub(crate) fn with_probe(mut self, probe: Option<Arc<dyn Session>>) -> Runner<F> {
        self.probe = probe;
        self
    }

    /// The runner with the 30 s pass's cap read from `clock`.
    #[cfg(test)]
    pub(crate) fn with_clock(mut self, clock: Arc<dyn Clock>) -> Runner<F> {
        self.clock = clock;
        self
    }

    /// The runner reporting its checks to `busy`, for a watchdog the tests drive.
    #[cfg(test)]
    pub(crate) fn with_busy(mut self, busy: Arc<watchdog::Busy>) -> Runner<F> {
        self.busy = busy;
        self
    }

    /// When each check begins and ends, for the watchdog.
    pub(crate) fn busy(&self) -> &Arc<watchdog::Busy> {
        &self.busy
    }

    /// Whether the stream was lost.
    #[cfg(test)]
    pub(crate) fn lost(&self) -> bool {
        self.lost.is_some()
    }

    /// Whether a process of the session may still run: the kill's cap ran out first.
    #[cfg(test)]
    pub(crate) fn lingering(&self) -> bool {
        self.lingering
    }

    /// One input (`None` for a tick) at `now`.
    pub(crate) fn input(&mut self, msg: Option<Msg>, now: Duration) -> Flow {
        self.busy.start();
        let found = self.check(msg, now);
        let overran = self.busy.stop();
        if (found || overran) && self.ending.is_none() {
            if overran && !found {
                self.watch.lost_stream();
            }
            self.watch.ended_session();
            self.ending = Some(now);
        }
        self.settle(now)
    }

    /// The checks of one input: true on the first find, the canary's miss included, at once,
    /// so the kill goes before any rescan or 30 s pass. Nothing is checked while the session
    /// is ending, nor live without a stream.
    fn check(&mut self, msg: Option<Msg>, now: Duration) -> bool {
        let mut rescan = false;
        match msg {
            Some(Msg::Events(events, queued)) => {
                let events = self.echoes(events);
                if self.ending.is_some() {
                    return false;
                }
                let lag = now.saturating_sub(queued);
                if lag >= CANARY_WAIT && self.mode == Mode::Live {
                    tracing::error!(
                        ?lag,
                        "the git watch reads the stream's events late; it lags behind the agent"
                    );
                    return self.lose(now);
                }
                // The whole batch at once: every name first, then the scans, on one budget.
                let asks = self.watch.events(&events);
                if asks.end {
                    return true;
                }
                rescan = asks.rescan;
            }
            Some(Msg::Attach(session)) => {
                self.busy.attach(&session);
                self.session = Some(session);
            }
            Some(Msg::Abandon) => self.abandoned = true,
            None => {}
        }
        if self.ending.is_some() || self.mode == Mode::AfterOnly {
            return false;
        }
        if self.canary(now) {
            return true;
        }
        if rescan && self.watch.full_scan(Phase::Running).end {
            return true;
        }
        if now >= self.next_sweep {
            let clock = Arc::clone(&self.clock);
            let until = clock.now() + SWEEP_CAP;
            let (found, _) = self.watch.sweep(&|| clock.now() >= until);
            self.next_sweep = now + SWEEP_EVERY;
            return found.end;
        }
        false
    }

    /// Whether the session is over, sending its kill again while it is ending.
    fn settle(&mut self, now: Duration) -> Flow {
        if self.abandoned {
            if self.session.is_none()
                && let Some(probe) = &self.probe
                && !probe.gone()
            {
                tracing::error!(
                    "the agent did not start, yet a process carrying its probe is left; \
                     moving nothing, and keeping its record so the matter's launches wait"
                );
                probe.end();
                self.lingering = true;
            }
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
                "the session's processes did not go after its kill; moving nothing, and \
                 keeping its record so the matter's launches wait until none is left"
            );
            self.lingering = true;
            Flow::Done
        } else {
            Flow::Go
        }
    }

    /// `events` without the canary's, which mark the stream as working, and without the rest
    /// of the set-aside folder's, which are not the watched folder's.
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
    /// event is late by the clock, judged on every input once the canary's events in the batch
    /// read with it count; or the canary can't be written. The lawyer is told then.
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
            let fs = self.watch.fs();
            let mut written = fs.touch(&canary.path);
            for _ in 1..CANARY_TRIES {
                if written.is_ok() {
                    break;
                }
                written = fs.touch(&canary.path);
            }
            match written {
                Ok(()) => {
                    canary.sent.get_or_insert(now);
                }
                Err(error) => {
                    tracing::error!(%error, "the git watch's canary can't be written");
                    lost = true;
                }
            }
        }
        lost && self.lose(now)
    }

    /// The stream is lost at `now`: the lawyer is told, once. Returns true.
    fn lose(&mut self, now: Duration) -> bool {
        tracing::error!(
            matter = %self.watch.matter().display(),
            "the git watch lost its FSEvents stream; ending the session"
        );
        if self.lost.is_none() {
            self.lost = Some(now);
            self.watch.lost_stream();
        }
        true
    }

    /// The final scan. With every process gone each find is set aside; with one left after the
    /// kill nothing moves: `outlived` marks the session's record first, before anything is
    /// read, then the finds are listed, within the scan's budget.
    pub(crate) fn finish(mut self, outlived: &dyn Fn()) -> End {
        if self.lingering {
            outlived();
            let finds = self.watch.finds();
            tracing::error!(
                matter = %self.watch.matter().display(), finds = finds.len(),
                "a process of the session outlived its kill; the final git scan moved nothing"
            );
            return End::Outlived(finds);
        }
        let asks = self.watch.full_scan(Phase::Ended);
        debug_assert!(!asks.end, "the final scan never asks to end");
        End::Scanned(self.watch.take_stuck())
    }
}
