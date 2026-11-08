//! The denial reporter's client (0049, "The denial reporter, client side"): the runner's half of
//! tier B's file-denial feed, as `macos/watch.rs` is on macOS. It runs in the lawyer's process,
//! unelevated, and holds one connection to the SYSTEM watcher (`setup::watch`) for the whole
//! host while any tier-B launch is registered.
//!
//! - **Registration:** each launch registers its pool user's SID, its `Policy` and its capped
//!   sink ([`register`]); dropping the [`Registration`] unregisters it. The first registration
//!   starts a supervisor thread and waits up to [`START_WAIT`] for its first attempt; the last
//!   one dropping makes the reader close the pipe within [`READ_WAIT`], and the watcher, with no
//!   client left, exits by itself.
//! - **Start:** the supervisor starts `\Clauseo\SandboxWatch` (`refresh::request_watch`), polls
//!   the published name (`protocol::WATCH_KEY`) every [`POLL_EVERY`] for up to [`PIPE_WAIT`],
//!   opens only a name `protocol::is_pipe_name` accepts, and checks the server before it reads
//!   a byte ([`verify`]): the pipe's owner is LOCAL SYSTEM, and, where Windows tells this user,
//!   the server runs in session 0, as LOCAL SYSTEM, from the admin copy.
//! - **Readiness:** live once connected; there is no probe. The watcher starts its trace and its
//!   consumer before it publishes the name, so a name the client can open is a feed already
//!   running. A probe would have to run something as the pool user: the host has no cheap way
//!   (each logon needs the pool password and a profile load), and the inner runner's order
//!   before the launch (M1) is kept free of anything else.
//! - **Records:** each line is `protocol::Line`. A refusal goes to the newest registration of
//!   its pool SID. Its access is the line's, and `unknown` (a plain open) is worked out with
//!   [`Watch::would_deny`]: a pool user allowed to read the path was refused a write, and
//!   anything else, a check that can't tell included, is a read. A line is local only when it
//!   sends `"net":false` itself (a missing `net` is a network path, whatever `Line`'s serde
//!   default says) and its path is `X:\...` ([`is_local_drive_path`]); anything else is
//!   for display only, opens nothing at all, and its `unknown` is a read. Then
//!   `policy.classify`, which compares strings and opens nothing, and [`Denial::Path`] into that
//!   launch's capped sink, its path shown through `display_safe_path` (each invisible or
//!   control character escaped). A `dropped` line carries no SID; it goes
//!   to the launch of the last refusal routed, or to every launch when none was. Sinks are
//!   ticked every [`TICK`].
//! - **The key and the server:** a name is trusted only from a key LOCAL SYSTEM owns; the pipe
//!   is opened with `GENERIC_READ` alone and identification-only quality of service, so a fake
//!   server can't act as the lawyer.
//! - **A second request:** the task ignores a run while one is running (`IGNORE_NEW`), so a
//!   request made as an old watcher was exiting is lost, and a watcher that exits 74 wants a
//!   fresh one. With no pipe open [`RETRIGGER_AFTER`] after the request, the task is asked once
//!   more, within the same [`PIPE_WAIT`].
//! - **A closed pipe** may have lost refusals (the watcher ended, or exited 0 because the record's
//!   owner changed): it is logged each time, `Denial` having no "unknown count", and the
//!   supervisor reconnects under the restart limit.
//! - **The IT off switch:** [`POLICY_VALUE`] a `DWORD` 0 in `HKLM\`[`POLICY_KEY`] means the
//!   task is never started: [`Down::TurnedOff`], logged once.
//! - **The breaker:** the watcher's `paused` line (heavy disk activity) reaches every registered
//!   launch once as [`Denial::ReporterPaused`](crate::Denial::ReporterPaused); `resumed`, or a
//!   new connection, clears it.
//! - **Fallback:** a task not registered, a watcher that exits 68 (the provider's sessions are
//!   taken) or anything else, no name within [`PIPE_WAIT`], or a key or server that fails the
//!   check marks the reporter [`Status::Down`] and is logged once per reason. Launches go
//!   ahead either way: the reporter only informs, and the host-side source keeps working. The
//!   supervisor tries again at most once every [`RESTART_EVERY`], as on macOS, and so after a
//!   pipe that closed.
//!
//! Privacy: the watcher sends only pool users' refusals. Nothing here logs a path or a SID; the
//! paths go only into `Denial`.
//!
//! Every effect goes through [`Watch`], which `os.rs` implements on Windows and the tests fake.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::sids::Sid;
use crate::{Access, CappedSink, Clock, DenialSink, Policy};

#[cfg(windows)]
mod os;
mod route;
mod supervise;
#[cfg(test)]
mod tests;
pub mod verify;
mod walk;

#[cfg(test)]
use route::Lines;
pub use route::is_local_drive_path;
pub use walk::{Walker, open_no_reparse};

#[cfg(windows)]
pub use os::would_deny;

/// A supervisor whose watcher failed to start, or whose pipe closed, starts it again at most
/// this often.
pub const RESTART_EVERY: Duration = Duration::from_secs(60);
/// How long the published name is polled for after the task was asked to run.
pub const PIPE_WAIT: Duration = Duration::from_secs(5);
/// When, after the first request, the task is asked once more if no pipe was opened yet: the
/// task ignores a run while one is running (`IGNORE_NEW`), so a request made as an old watcher
/// was exiting, or one that exited 74, needs a second.
pub const RETRIGGER_AFTER: Duration = Duration::from_secs(2);
/// How often the published name is read while it is polled.
pub const POLL_EVERY: Duration = Duration::from_millis(100);
/// How long the first registration waits for the supervisor's first attempt.
pub const START_WAIT: Duration = Duration::from_secs(6);
/// The longest one read of the pipe waits, so an unregistration is seen this soon.
pub const READ_WAIT: Duration = Duration::from_millis(250);
/// How often every registered sink's rate cap is ticked.
pub const TICK: Duration = Duration::from_secs(1);
/// The longest line kept; a longer one is skipped to its `\n`.
pub const LINE_CAP: usize = 256 * 1024;
/// The policy key IT may set to turn the reporter off, under `HKLM`.
pub const POLICY_KEY: &str = r"SOFTWARE\Policies\Clauseo\Sandbox";
/// Its `DWORD` value: 0 turns the reporter off; anything else, or none, leaves it on.
pub const POLICY_VALUE: &str = "DenialReporter";
/// The watcher's exit code when the provider's sessions are taken (`WatchExit::ProviderFull`).
pub const PROVIDER_FULL: u32 = 68;

/// A launch's sink with its rate cap, which the client ticks every [`TICK`].
pub trait Capped: DenialSink {
    /// Delivers the cap's held-back count once its window has ended.
    fn tick(&self);
}

impl<S: DenialSink, C: Clock> Capped for CappedSink<S, C> {
    fn tick(&self) {
        CappedSink::tick(self);
    }
}

/// Why the reporter is down. Appended, never reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Down {
    /// `\Clauseo\SandboxWatch` isn't registered (no admin copy, or an older set-up).
    NotRegistered,
    /// The watcher exited 68: the file provider's 8 sessions are taken.
    ProviderFull,
    /// No pipe name the client could open was published within [`PIPE_WAIT`].
    NoPipe,
    /// A published pipe's server failed the check ([`verify`]).
    NotTheServer,
    /// Starting the task or opening the pipe failed otherwise.
    Failed,
    /// The pipe closed or its read failed; the supervisor starts it again.
    Ended,
    /// IT turned it off: [`POLICY_VALUE`] in `HKLM\`[`POLICY_KEY`] is a `DWORD` 0. The task is
    /// never started.
    TurnedOff,
}

/// The reporter's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// No launch is registered.
    #[default]
    Off,
    /// The supervisor's first attempt hasn't ended.
    Starting,
    /// Connected to a checked server: refusals are reported.
    Live,
    /// Not reporting, and why; the host-side source still reports.
    Down(Down),
}

/// Why the task couldn't be asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// It isn't registered.
    NotRegistered,
    /// Anything else, as a line for the log.
    Failed(String),
}

/// Why a published pipe couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// No pipe has the name (yet), or all its instances are busy: poll again.
    Absent,
    /// Its server failed the check, as a line for the log.
    NotTheServer(String),
    /// Anything else, as a line for the log.
    Failed(String),
}

/// The pipe's client end, as the reader uses it.
pub trait Pipe: Send + 'static {
    /// Reads into `buf`, waiting up to `wait`: `Some(0)` at the end, `None` when nothing came.
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<Option<usize>>;
}

/// Every effect of the client.
pub trait Watch: Send + Sync + 'static {
    /// The connected pipe.
    type Pipe: Pipe;
    /// Asks the Task Scheduler to run `\Clauseo\SandboxWatch` (`refresh::request_watch`).
    fn request(&self) -> Result<(), StartError>;
    /// The published pipe name, if one is there; `Err` when the key's owner isn't LOCAL SYSTEM
    /// or can't be read, and so its name isn't trusted.
    fn pipe_name(&self) -> Result<Option<String>, String>;
    /// Opens the pipe `name` and checks its server ([`verify::check_server`]).
    fn connect(&self, name: &str) -> Result<Self::Pipe, ConnectError>;
    /// Whether IT turned the reporter off: [`POLICY_VALUE`] in `HKLM\`[`POLICY_KEY`] (64-bit
    /// view) is a `DWORD` 0. Read before each attempt, so a change applies at the next.
    fn turned_off(&self) -> bool;
    /// The task's last result, when it can be read.
    fn last_exit(&self) -> Option<u32>;
    /// Whether the path's descriptor refuses `access` to `pool_user`; `None` when it can't tell.
    fn would_deny(&self, pool_user: &Sid, path: &Path, access: Access) -> Option<bool>;
    /// The time since some fixed start, from a monotonic clock.
    fn now(&self) -> Duration;
    /// Waits `wait`.
    fn sleep(&self, wait: Duration);
    /// Writes one line to the log, `error` for a failure. Never a path or a SID.
    fn log(&self, error: bool, line: &str);
}

/// One registered launch.
struct Entry {
    pool_user: Sid,
    policy: Arc<Policy>,
    sink: Arc<dyn Capped>,
}

/// The client's shared state.
#[derive(Default)]
struct State {
    launches: BTreeMap<u64, Entry>,
    next_id: u64,
    /// Bumped each time a registration finds none; a supervisor whose generation is stale
    /// stops, and its reader closes the pipe.
    generation: u64,
    status: Status,
    /// The reason last logged, so a reason is logged once until the reporter is live again.
    logged: Option<Down>,
    /// The registration the last refusal went to, for a `dropped` line.
    last_routed: Option<u64>,
    /// The watcher's breaker is open: its pause was told, and isn't told again until it resumes.
    paused: bool,
}

/// The client: its effects and its state.
struct Shared<W: Watch> {
    watch: W,
    state: Mutex<State>,
}

/// The client for one [`Watch`]. The real one is a static on Windows ([`register`]).
pub struct Hub<W: Watch> {
    shared: Arc<Shared<W>>,
}

/// What a registration unregisters through, whatever the [`Watch`].
trait Unregister: Send + Sync {
    fn unregister(&self, id: u64);
}

/// A launch's place in the client; dropping it unregisters the launch, and the last drop closes
/// the pipe.
pub struct Registration {
    shared: Arc<dyn Unregister>,
    id: u64,
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.shared.unregister(self.id);
    }
}

impl<W: Watch> Unregister for Shared<W> {
    fn unregister(&self, id: u64) {
        let mut state = self.lock();
        state.launches.remove(&id);
        if state.last_routed == Some(id) {
            state.last_routed = None;
        }
        if state.launches.is_empty() {
            state.status = Status::Off;
        }
    }
}

impl<W: Watch> Hub<W> {
    /// A client over `watch`, with nothing registered.
    pub fn new(watch: W) -> Hub<W> {
        Hub {
            shared: Arc::new(Shared {
                watch,
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// Registers a launch run as `pool_user` under `policy`, whose denials go to `sink`. The
    /// first registration starts the supervisor and waits up to [`START_WAIT`] for its first
    /// attempt, live or down; the launch goes ahead either way.
    pub fn register(
        &self,
        pool_user: Sid,
        policy: Arc<Policy>,
        sink: Arc<dyn Capped>,
    ) -> Registration {
        let (id, first) = {
            let mut state = self.shared.lock();
            let id = state.next_id;
            state.next_id += 1;
            let first = state.launches.is_empty();
            let entry = Entry {
                pool_user,
                policy,
                sink,
            };
            state.launches.insert(id, entry);
            let first = first.then(|| {
                state.generation += 1;
                state.status = Status::Starting;
                state.generation
            });
            (id, first)
        };
        if let Some(generation) = first {
            self.start(generation);
        }
        Registration {
            shared: Arc::clone(&self.shared) as Arc<dyn Unregister>,
            id,
        }
    }

    /// The reporter's state now.
    pub fn status(&self) -> Status {
        self.shared.lock().status
    }

    /// Starts the supervisor for `generation` and waits for its first attempt.
    fn start(&self, generation: u64) {
        let (started, wait) = mpsc::channel();
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("clauseo-sandbox-watch-client".to_owned())
            .spawn(move || supervise::supervise(&shared, generation, started));
        match spawned {
            Ok(_) => {
                if wait.recv_timeout(START_WAIT).is_err() {
                    let line = "the denial reporter's first attempt didn't end in time";
                    self.shared.watch.log(false, line);
                }
            }
            Err(error) => {
                let line = format!("the denial reporter's thread didn't start: {error}");
                self.shared.watch.log(true, &line);
            }
        }
    }
}

impl<W: Watch> Shared<W> {
    fn lock(&self) -> MutexGuard<'_, State> {
        // The registrations stay valid whatever a panicking thread left.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `generation` is the running one and a launch is registered.
    fn current(&self, generation: u64) -> bool {
        let state = self.lock();
        state.generation == generation && !state.launches.is_empty()
    }

    /// Marks `generation` live, and logs it when it was down.
    fn live(&self, generation: u64) {
        let was_down = {
            let mut state = self.lock();
            if state.generation == generation && !state.launches.is_empty() {
                state.status = Status::Live;
            }
            state.paused = false;
            state.logged.take().is_some()
        };
        let line = if was_down {
            "the denial reporter is live again"
        } else {
            "the denial reporter is live"
        };
        self.watch.log(false, line);
    }

    /// Marks `generation` down for `down`, logging `line` unless that reason was logged last;
    /// a closed pipe is logged each time (at most once a minute), as refusals may be lost.
    fn down(&self, generation: u64, down: Down, line: &str) {
        let first = {
            let mut state = self.lock();
            if state.generation == generation && !state.launches.is_empty() {
                state.status = Status::Down(down);
            }
            let first = down == Down::Ended || state.logged != Some(down);
            state.logged = Some(down);
            first
        };
        if first {
            self.watch.log(true, line);
        }
    }
}

/// The capped sink one launch's denials go through on the real clock.
#[cfg(windows)]
type LaunchSink = CappedSink<Arc<dyn DenialSink>>;

/// The host's client.
#[cfg(windows)]
static HUB: std::sync::OnceLock<Hub<os::OsWatch>> = std::sync::OnceLock::new();

#[cfg(windows)]
fn hub() -> &'static Hub<os::OsWatch> {
    HUB.get_or_init(|| Hub::new(os::OsWatch::new()))
}

/// Registers a tier-B launch run as `pool_user` under `policy`; its denials reach `denials`
/// through the rate cap. `Launchers::launch`'s launcher calls it once, on its first launch, and
/// keeps the [`Registration`] while the agent runs, as `SeatbeltLauncher` does. The first
/// registration may wait up to [`START_WAIT`]: call it off the UI thread.
#[cfg(windows)]
pub fn register(pool_user: Sid, policy: Arc<Policy>, denials: Arc<dyn DenialSink>) -> Registration {
    let sink: Arc<dyn Capped> = Arc::new(LaunchSink::new(denials));
    hub().register(pool_user, policy, sink)
}

/// The host's reporter's state.
#[cfg(windows)]
pub fn status() -> Status {
    hub().status()
}
