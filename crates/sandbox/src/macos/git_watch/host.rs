//! What the host calls around the git watches of its launches (decision record 0038): once at
//! its start, [`start_up`] finishes every matter's sessions that a quit or crash left without
//! their final scan; at its quit, [`quit`] kills every running session by its probe and waits
//! a little for their final scans. Also the host's register of its running watches.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::os;
use super::state::{self, FileStore, Host, SessionRecord};
use crate::macos::jail_procs::{self, Probe};
use crate::{Clock, DenialSink, SystemClock};

/// How long a launch, or the host's start, waits for a dead session's processes to go.
const DEAD_KILL_WAIT: Duration = Duration::from_secs(3);

/// How often [`quit`] sends its kills again while a watch runs.
const QUIT_AGAIN: Duration = Duration::from_millis(50);

/// The running watches of one host, by session, each with its session's kill.
pub(crate) struct Register {
    live: Mutex<BTreeMap<String, Arc<dyn Fn() + Send + Sync>>>,
    quitting: AtomicBool,
}

/// This host's.
pub(crate) static LIVE: Register = Register::new();

impl Register {
    pub(crate) const fn new() -> Register {
        Register {
            live: Mutex::new(BTreeMap::new()),
            quitting: AtomicBool::new(false),
        }
    }

    fn map<T>(&self, f: impl FnOnce(&mut BTreeMap<String, Arc<dyn Fn() + Send + Sync>>) -> T) -> T {
        f(&mut self.live.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Adds session `session`'s watch with its `kill`; refused once the host quits.
    pub(crate) fn add(&self, session: &str, kill: Arc<dyn Fn() + Send + Sync>) -> io::Result<()> {
        self.map(|live| {
            if self.quitting.load(Ordering::SeqCst) {
                return Err(io::Error::other("Clauseo is quitting"));
            }
            live.insert(session.to_owned(), kill);
            Ok(())
        })
    }

    /// Takes session `session`'s watch off, once its final scan is done or it never started.
    pub(crate) fn remove(&self, session: &str) {
        self.map(|live| live.remove(session));
    }

    /// Whether session `session`'s watch runs in this host.
    pub(crate) fn has(&self, session: &str) -> bool {
        self.map(|live| live.contains_key(session))
    }

    /// Refuses every later [`Register::add`]; kills every session left, again every 50 ms,
    /// until each watch has run its final scan and gone, or `within` (on `clock`) is out.
    /// Returns whether every watch went in time.
    pub(crate) fn quit(&self, within: Duration, clock: &dyn Clock) -> bool {
        self.quitting.store(true, Ordering::SeqCst);
        let until = clock.now() + within;
        loop {
            let kills: Vec<_> = self.map(|live| live.values().cloned().collect());
            if kills.is_empty() {
                return true;
            }
            for kill in &kills {
                kill();
            }
            if clock.now() >= until {
                tracing::warn!(
                    left = kills.len(),
                    "Clauseo quits before every git watch's final scan; its next start runs them"
                );
                return false;
            }
            async_io::block_on(async_io::Timer::after(QUIT_AGAIN));
        }
    }
}

/// Whether recorded session `record` never ran a final scan that moves: a process of it
/// outlived its kill, its host is gone (no process with its pid and start), or it is this host
/// and its watch isn't running.
pub(crate) fn dead(record: &SessionRecord) -> bool {
    if record.outlived {
        return true;
    }
    let this = jail_procs::this_process();
    if (record.host_pid, record.host_start) == this {
        !LIVE.has(&record.session)
    } else {
        jail_procs::start_of(record.host_pid).is_none_or(|s| Some(s) != record.host_start)
    }
}

/// Ends every process a dead session left, by its probe name, waiting up to 3 s for them.
/// Only a process started at or after the session's launch is killed: the probe's random part
/// already keeps another host run's sessions out, and its start keeps out whatever else
/// carries the name. Returns whether none is left.
pub(crate) fn kill_dead(record: &SessionRecord) -> bool {
    let name = record.probe.as_str();
    let Some(probe) = Probe::from_name(name) else {
        tracing::error!(name, "a recorded probe name Clauseo never makes");
        return true;
    };
    let probe = probe.since(record.launched);
    let clock = SystemClock::new();
    let until = clock.now() + DEAD_KILL_WAIT;
    while !probe.gone() && clock.now() < until {
        probe.kill_all();
        async_io::block_on(async_io::Timer::after(Duration::from_millis(50)));
    }
    let gone = probe.gone();
    if !gone {
        tracing::error!(
            name,
            "a dead session's processes did not go; nothing moves while one is left"
        );
    }
    gone
}

/// What a launch and the host's start pass the state's dead-session sweep.
pub(crate) fn host() -> Host<'static> {
    Host {
        dead: &dead,
        kill: &kill_dead,
    }
}

/// The macOS git watch's work at the host's start. Call it once, before or alongside the first
/// launch, on a thread of its own: it blocks while it works. Under each matter's lock, for
/// every matter whose state file is in `git_set_aside` (the folder the host passes
/// [`PolicyBuilder::git_set_aside`](crate::PolicyBuilder::git_set_aside)), it finds the sessions
/// whose host quit or crashed before their final scan, kills what is left of each by its
/// probe (waiting up to 3 s), and once none is left runs that final scan, setting aside what
/// the agent left and reporting each to `notices` as [`Denial::GitSetAside`](crate::Denial).
/// A launch of a matter does the same for that matter first, so a launch racing this is safe.
/// A record stays while a process of it is left (nothing of it moves then, and the matter's
/// launches are refused) or its matter is missing (a disk not plugged in), for the next try.
/// The notes folders of sessions have state files of their own here, and get the same.
/// Returns how many sessions' scans ran; an error only when the state folder can't be listed
/// (a missing one is none). One matter's error is logged and the rest go on.
pub fn start_up(git_set_aside: &Path, notices: &Arc<dyn DenialSink>) -> io::Result<usize> {
    super::run::remove_stale_canaries(&os::Real, git_set_aside);
    let dir = git_set_aside.join("matters");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut scanned = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let store = FileStore::at(&path);
        // The matter's own volume, read from its folder now: an after-only matter's is let in.
        let fs = store
            .root()
            .and_then(|root| os::Within::of(&root).ok())
            .unwrap_or_default();
        match state::sweep_dead(&store, &fs, notices, &host()) {
            Ok(n) => scanned += n,
            Err(error) => {
                tracing::error!(file = %path.display(), %error, "a matter's git state can't be read");
            }
        }
    }
    Ok(scanned)
}

/// The macOS git watch's work at the host's quit: refuses every later launch, kills every
/// process of every running session by its probe (again every 50 ms), and waits up to
/// `within` for each session's watch to run its final scan and stop. Call it once, from the
/// thread that quits, before the process exits. Returns whether every watch finished; one
/// that didn't keeps its record, and [`start_up`] at the next start kills and scans for it.
pub fn quit(within: Duration) -> bool {
    LIVE.quit(within, &SystemClock::new())
}
