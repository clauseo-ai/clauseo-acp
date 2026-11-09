//! The watchdog: one thread per host that ends a session whose watch is stuck in one check.
//!
//! The watch reads what the agent made while the agent runs: a link's target is resolved, a
//! folder moved in is walked whole. A link into `/net/...` makes autofs mount it, and a slow
//! server holds the watch's thread for as long as the mount takes; a huge folder holds it for
//! as long as its walk. While it is held, no event is checked and the canary isn't read, so
//! the agent runs unwatched. So each runner says when the check it is in began ([`Busy`]), and
//! a walk says each folder it lists ([`Busy::beat`]); every 100 ms the watchdog looks: a check
//! with no progress for [`BUDGET`] ends the session (its group, its tree and every process
//! carrying its probe), logged, and the kill goes again on each look while that check runs. A
//! rescan of a big matter that keeps listing folders is not stuck, however long it takes. The
//! runner, once its check returns, finds the session ended, tells the lawyer the watch was
//! lost, and goes on to the final scan. A check still held `KILL_CAP` after the first kill,
//! with the session's processes left, has the session's record marked outlived from the
//! watchdog's thread: the matter's next launch is refused even if the check never returns.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use super::{KILL_CAP, Session};
use crate::{Clock, SystemClock};

/// The most time one check may go without progress while the agent may run. 2 s is how long
/// the canary's event may take before the stream counts as lost (`CANARY_WAIT`): a check held
/// longer leaves the agent as unwatched as a lost stream does. A check that is not stuck lists
/// a folder well within it on a local disk; one that doesn't, such as a listing on a disk that
/// stopped answering, ends the session (fail-closed).
pub(crate) const BUDGET: Duration = Duration::from_secs(2);

/// How often the watchdog looks.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// What marks a session's record outlived.
type Mark = Box<dyn Fn() + Send + Sync>;

/// One runner's state as the watchdog sees it: when its check last made progress, whether and
/// when the watchdog ended its session, and the session.
pub(crate) struct Busy {
    clock: Arc<dyn Clock>,
    /// When the check now running began or last made progress, in milliseconds on `clock`
    /// plus one; 0 between checks.
    since: AtomicU64,
    /// The watchdog ended the session. Stays set.
    overran: AtomicBool,
    /// How many checks began.
    checks: AtomicU64,
    /// When the watchdog first ended the session, as `since`, and in which check.
    overran_at: AtomicU64,
    overran_in: AtomicU64,
    /// The session's record was marked outlived. Stays set.
    marked: AtomicBool,
    /// Marks the session's record outlived.
    mark: Mutex<Option<Mark>>,
    session: Mutex<Option<Arc<dyn Session>>>,
}

impl Busy {
    /// A runner's, read on `clock`.
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Arc<Busy> {
        Arc::new(Busy {
            clock,
            since: AtomicU64::new(0),
            overran: AtomicBool::new(false),
            checks: AtomicU64::new(0),
            overran_at: AtomicU64::new(0),
            overran_in: AtomicU64::new(0),
            marked: AtomicBool::new(false),
            mark: Mutex::new(None),
            session: Mutex::new(None),
        })
    }

    fn millis(&self) -> u64 {
        u64::try_from(self.clock.now().as_millis())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }

    /// A check begins.
    pub(crate) fn start(&self) {
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.since.store(self.millis(), Ordering::SeqCst);
    }

    /// The check now running made progress (a walk listed a folder): its budget starts again.
    /// Between checks, nothing. Returns whether a check runs. Test-only until
    /// `Watch::on_progress` calls it (W3's merge).
    #[cfg(test)]
    pub(crate) fn beat(&self) -> bool {
        let now = self.millis();
        self.since
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |since| {
                (since != 0).then_some(now)
            })
            .is_ok()
    }

    /// `mark` marks the session's record outlived, for a check stuck past `KILL_CAP`.
    pub(crate) fn on_stuck(&self, mark: impl Fn() + Send + Sync + 'static) {
        *self.mark.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(mark));
    }

    /// The check is over. Returns whether the watchdog has ended the session, during this
    /// check or an earlier one.
    pub(crate) fn stop(&self) -> bool {
        self.since.store(0, Ordering::SeqCst);
        self.overran.load(Ordering::SeqCst)
    }

    /// The agent runs as `session`. When the watchdog already gave up on a check, which can
    /// happen before the agent starts, it is ended at once.
    pub(crate) fn attach(&self, session: &Arc<dyn Session>) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(session));
        if self.overran.load(Ordering::SeqCst) {
            session.end();
        }
    }

    /// The session, once attached.
    pub(crate) fn session(&self) -> Option<Arc<dyn Session>> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The watchdog's look: when the check running made no progress for `budget`, ends the
    /// session. Once it has, the kill goes again on each look while that check runs, and once
    /// the check it ended in has run `KILL_CAP` past that with a process of the session left,
    /// the session's record is marked outlived, once. Returns whether a check is judged stuck.
    pub(crate) fn look(&self, budget: Duration) -> bool {
        let since = self.since.load(Ordering::SeqCst);
        if since == 0 {
            return false;
        }
        let now = self.millis();
        let session = self.session();
        if self.overran.load(Ordering::SeqCst) {
            let at = self.overran_at.load(Ordering::SeqCst);
            let held = Duration::from_millis(now.saturating_sub(at));
            let same = self.checks.load(Ordering::SeqCst) == self.overran_in.load(Ordering::SeqCst);
            if let Some(session) = &session {
                session.end();
                if same
                    && held >= KILL_CAP
                    && !session.gone()
                    && !self.marked.swap(true, Ordering::SeqCst)
                {
                    tracing::error!(
                        ?held,
                        "a git watch check is still stuck and the session's processes outlive \
                         its kill; marking its record so the matter's launches wait"
                    );
                    if let Some(mark) = &*self.mark.lock().unwrap_or_else(PoisonError::into_inner) {
                        mark();
                    }
                }
            }
            return true;
        }
        let idle = Duration::from_millis(now.saturating_sub(since));
        if idle < budget {
            return false;
        }
        self.overran_at.store(now, Ordering::SeqCst);
        self.overran_in
            .store(self.checks.load(Ordering::SeqCst), Ordering::SeqCst);
        self.overran.store(true, Ordering::SeqCst);
        tracing::error!(
            ?idle,
            "a git watch check is stuck; ending the session, which runs unwatched meanwhile"
        );
        if let Some(session) = &session {
            session.end();
        }
        true
    }
}

/// The runners the watchdog looks at.
static WATCHED: Mutex<Vec<Weak<Busy>>> = Mutex::new(Vec::new());

/// Puts `busy` under the host's watchdog, started on the first call. Refused when the
/// watchdog's thread can't start: the agent never runs with a watch nothing watches.
pub(crate) fn watch(busy: &Arc<Busy>) -> io::Result<()> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    let started = STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("clauseo-git-watchdog".to_owned())
            .spawn(|| {
                run();
            })
            .map(drop)
            .map_err(|error| error.to_string())
    });
    if let Err(error) = started {
        return Err(io::Error::other(format!(
            "the git watch's watchdog did not start: {error}"
        )));
    }
    WATCHED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Arc::downgrade(busy));
    Ok(())
}

/// The watchdog's thread: looks at every runner every 100 ms, for the host's life.
fn run() -> ! {
    loop {
        async_io::block_on(async_io::Timer::after(LOOK_EVERY));
        let busy: Vec<Arc<Busy>> = {
            let mut watched = WATCHED.lock().unwrap_or_else(PoisonError::into_inner);
            watched.retain(|busy| busy.strong_count() > 0);
            watched.iter().filter_map(Weak::upgrade).collect()
        };
        for busy in busy {
            busy.look(BUDGET);
        }
    }
}

/// The clock a runner's [`Busy`] reads when none is given.
pub(crate) fn clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock::new())
}
