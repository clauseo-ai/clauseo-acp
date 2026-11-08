//! The watchdog: one thread per host that ends a session whose watch is stuck in one check.
//!
//! The watch reads what the agent made while the agent runs: a link's target is resolved, a
//! folder moved in is walked whole. A link into `/net/...` makes autofs mount it, and a slow
//! server holds the watch's thread for as long as the mount takes; a huge folder holds it for
//! as long as its walk. While it is held, no event is checked and the canary isn't read, so
//! the agent runs unwatched. So each runner says when the check it is in began ([`Busy`]),
//! and every 100 ms the watchdog looks: a check that began [`BUDGET`] or more ago ends the
//! session (its group, its tree and every process carrying its probe), logged. The runner,
//! once its check returns, finds the session ended, tells the lawyer the watch was lost, and
//! goes on to the final scan.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use super::Session;
use crate::{Clock, SystemClock};

/// The most time one check may take while the agent may run. 2 s is how long the canary's
/// event may take before the stream counts as lost (`CANARY_WAIT`): a check held longer leaves
/// the agent as unwatched as a lost stream does. A check that is not stuck stays well under
/// it on a local disk (the 30 s pass stops at 250 ms); one that doesn't, such as the walk of a
/// folder of some hundred thousand entries moved in at once, ends the session (fail-closed).
pub(crate) const BUDGET: Duration = Duration::from_secs(2);

/// How often the watchdog looks.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// One runner's state as the watchdog sees it: when its check began, whether the watchdog
/// ended its session, and the session.
pub(crate) struct Busy {
    clock: Arc<dyn Clock>,
    /// When the check now running began, in milliseconds on `clock` plus one; 0 between
    /// checks.
    since: AtomicU64,
    /// The watchdog ended the session. Stays set.
    overran: AtomicBool,
    session: Mutex<Option<Arc<dyn Session>>>,
}

impl Busy {
    /// A runner's, read on `clock`.
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Arc<Busy> {
        Arc::new(Busy {
            clock,
            since: AtomicU64::new(0),
            overran: AtomicBool::new(false),
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
        self.since.store(self.millis(), Ordering::SeqCst);
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

    /// The watchdog's look: when the check running began `budget` or more ago, ends the
    /// session, once. Returns whether it did.
    pub(crate) fn look(&self, budget: Duration) -> bool {
        let since = self.since.load(Ordering::SeqCst);
        if since == 0 {
            return false;
        }
        let took = Duration::from_millis(self.millis().saturating_sub(since));
        if took < budget || self.overran.swap(true, Ordering::SeqCst) {
            return false;
        }
        tracing::error!(
            ?took,
            "a git watch check is stuck; ending the session, which runs unwatched meanwhile"
        );
        if let Some(session) = self.session() {
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
