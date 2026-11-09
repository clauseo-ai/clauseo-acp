//! `clauseo-sandbox-setup.exe --watch` (0049, "The denial reporter, SYSTEM side"), run by the
//! on-demand SYSTEM task `\Clauseo\SandboxWatch`: reports the files the kernel refused a pool
//! user, to the record owner's runner, over a pipe only that owner can open.
//!
//! [`watch`] holds the order and the refusals, with no Windows call; every effect goes through
//! [`Watcher`], which `windows_watcher.rs` implements and the tests fake. It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path (70);
//! 2. reads `PoolSids` and `OwnerSid` from `HKLM\SOFTWARE\Clauseo\Sandbox`, which only the
//!    elevated set-up writes (71 when missing or unreadable). The task passes `--watch` alone:
//!    nothing the lawyer gives reaches this process. An empty pool is nothing to watch (0);
//! 3. holds every pool SID to the pool-user check, as `--stop` does (72);
//! 4. starts the real-time session [`SESSION_NAME`], first stopping one of that name a crash
//!    left, and enables `Microsoft-Windows-Kernel-File` for Create and `OperationEnd` with each
//!    event's user SID (`windows_trace.rs`). A provider whose 8 sessions are taken is
//!    [`WatchExit::ProviderFull`] (68): the runner falls back to its host-side source. Any other
//!    failure is 73;
//! 5. makes a pipe with a random name, only the owner may open it, and publishes its name in
//!    [`protocol::WATCH_KEY`], a key only SYSTEM may write (74 on failure). A key a killed run
//!    left (SYSTEM's) is replaced; anyone else's is refused (74);
//! 6. serves ([`serve`]): the consumer (`consumer.rs`) drops every Create whose SID isn't a pool
//!    user's on arrival, pairs a pool Create with the next `OperationEnd` of its IRP, and passes
//!    on refusals (`STATUS_ACCESS_DENIED`) as JSON lines, at most
//!    [`protocol::RECORDS_PER_SECOND`], to every client. The pipe's instances never reach zero
//!    (`instances.rs`); a later instance that can't be made, or isn't SYSTEM's, ends the run
//!    with 74. Every [`serve::OWNER_CHECK`] it reads the record again: another owner (or none)
//!    ends the run with 0, so it never serves a previous owner after a new set-up. The
//!    event-rate breaker (`breaker.rs`) disables the provider for a minute after ten seconds
//!    at or above [`breaker::PAUSE_RATE`] events a second, and tells the clients;
//! 7. exits once the last client has gone, after [`lifetime::LAST_CLIENT_GRACE`], or when none
//!    came within [`lifetime::FIRST_CLIENT_WAIT`]: it stops the session, deletes the published
//!    name and exits 0. A trace that ends on its own is 75.
//!
//! Privacy: a non-pool event is dropped in the callback, before anything is stored, logged or
//! sent; the output channel only ever sees a pool user's refusal, and the writer checks the
//! SID again before a line goes out. The log lines carry counts only: no path, no SID.

use std::path::PathBuf;
use std::time::Duration;

use crate::windows::refresh::same_path;
use crate::windows::sids::Sid;

pub use super::stop::Pool;
pub use consumer::{Access, Consumer, Record};
pub use lifetime::{Lifetime, Verdict};
pub use serve::serve;

pub mod breaker;
pub mod consumer;
pub mod instances;
pub mod lifetime;
pub mod losses;
pub mod paths;
pub mod protocol;
mod serve;
pub mod teardown;
#[cfg(test)]
mod tests;
#[cfg(windows)]
pub(crate) mod windows_pipe;
#[cfg(windows)]
pub(crate) mod windows_trace;
#[cfg(windows)]
pub(crate) mod windows_watcher;

/// The real-time session's fixed name: a crash leaves at most this one, which the next start,
/// `--remove` and the task's removal stop by name.
pub const SESSION_NAME: &str = "ClauseoSandboxWatch";

/// The watcher's exit codes. 68, then 70 on: clear of the refresh's 32 to 44, the stop's 45 to
/// 50, and the codes the other `\Clauseo` tasks hold (51 to 67). Appended, never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchExit {
    /// 0: served until the last client left (or none came), the record's owner changed, or the
    /// pool is empty.
    Done,
    /// 68: the provider's 8 sessions are taken (`ERROR_NO_SYSTEM_RESOURCES`); the runner falls
    /// back to its host-side source.
    ProviderFull,
    /// 70: not LOCAL SYSTEM, or not run from the admin copy's path.
    NotAllowed,
    /// 71: there is no record, it can't be read, or its `PoolSids` or `OwnerSid` is malformed.
    NoRecord,
    /// 72: a SID `PoolSids` lists isn't a pool user's.
    NotPool,
    /// 73: the session couldn't be started, the provider enabled or the consumer opened.
    NoTrace,
    /// 74: the pipe couldn't be made, its name published, or its clients polled; a later
    /// instance couldn't be made or isn't SYSTEM's; or the published key is someone else's.
    NoPipe,
    /// 75: the trace ended while the watcher served.
    TraceLost,
}

impl WatchExit {
    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            WatchExit::Done => 0,
            WatchExit::ProviderFull => 68,
            WatchExit::NotAllowed => 70,
            WatchExit::NoRecord => 71,
            WatchExit::NotPool => 72,
            WatchExit::NoTrace => 73,
            WatchExit::NoPipe => 74,
            WatchExit::TraceLost => 75,
        }
    }
}

/// A failed effect: what failed, as a line for the log (never a path or a SID).
pub type Failed = String;

/// Why the trace didn't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceError {
    /// The provider's sessions are all taken.
    ProviderFull(Failed),
    /// Anything else.
    Other(Failed),
}

/// What the consumer gave since the last call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Pool users' refusals, and how many the consumer's bounded channel had no room for.
    Records(Vec<Record>, u64),
    /// The trace has ended: the consumer thread is gone.
    Ended,
}

/// Every effect of the watcher. The real one reads the record and the admin copy's path itself,
/// so no method takes a SID the caller chose, but the pool the record gave.
pub trait Watcher {
    /// Whether this process's token user is LOCAL SYSTEM.
    fn is_system(&mut self) -> Result<bool, Failed>;
    /// This process's exe, its final path.
    fn current_exe(&mut self) -> Result<PathBuf, Failed>;
    /// The admin copy's path.
    fn admin_copy(&mut self) -> Result<PathBuf, Failed>;
    /// The machine record's `OwnerSid` and `PoolSids`; `None` when there is no record.
    fn pool(&mut self) -> Result<Option<Pool>, Failed>;
    /// Whether every SID of `pool.users` passes the pool-user check.
    fn check_pool(&mut self, pool: &Pool) -> Result<(), Failed>;
    /// Stops a session of [`SESSION_NAME`] left behind, starts it, enables the provider and
    /// starts the consumer, which keeps only `pool`'s events.
    fn start_trace(&mut self, pool: &[Sid]) -> Result<(), TraceError>;
    /// Stops the session and waits for the consumer thread.
    fn stop_trace(&mut self);
    /// Makes the pipe, which only `owner` may open (read only), under a random name, and
    /// publishes the name.
    fn open_pipe(&mut self, owner: &Sid) -> Result<(), Failed>;
    /// Deletes the published name and closes every instance of the pipe.
    fn close_pipe(&mut self);
    /// How many Kernel-File events the callback has seen since the trace began.
    fn events(&mut self) -> u64;
    /// The breaker tripped: disables the provider on the session; the consumer clears its
    /// pending Creates before it reads the next event.
    fn pause_trace(&mut self) -> Result<(), Failed>;
    /// The pause ended: enables the provider again.
    fn resume_trace(&mut self) -> Result<(), Failed>;
    /// Whether `HKLM\SOFTWARE\Policies\Clauseo\Sandbox\WatchRateLog` is 1: log each second's
    /// event rate (a number only).
    fn rate_log(&mut self) -> bool;
    /// Waits up to `wait` for the consumer, then gives what it produced.
    fn records(&mut self, wait: Duration) -> Delivery;
    /// Takes clients that connected, drops those that left, and gives how many are connected.
    fn clients(&mut self) -> Result<usize, Failed>;
    /// Writes `line` to every connected client; one whose write fails or stalls is dropped.
    /// `Err` when the pipe can't keep an instance open (`instances.rs`): the run ends, 74.
    fn send(&mut self, line: &str) -> Result<(), Failed>;
    /// The time since the watcher began, from a monotonic clock.
    fn now(&mut self) -> Duration;
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// A refusal or failure: its exit and its log line.
type Refusal = (WatchExit, String);

/// Runs the watcher as the module says.
pub fn watch(w: &mut impl Watcher) -> WatchExit {
    match run_steps(w) {
        Ok(line) => {
            w.log(false, &format!("the sandbox's watcher: {line}"));
            WatchExit::Done
        }
        Err((exit, line)) => {
            w.log(true, &format!("the sandbox's watcher: {line}"));
            exit
        }
    }
}

/// The steps; `Ok` carries the log line of a run that ended as it should.
fn run_steps(w: &mut impl Watcher) -> Result<String, Refusal> {
    allowed(w)?;
    let pool = match w.pool().map_err(|why| (WatchExit::NoRecord, why))? {
        Some(pool) if !pool.users.is_empty() => pool,
        Some(_) => return Ok("no pool users are recorded".to_owned()),
        None => return Err((WatchExit::NoRecord, "there is no record".to_owned())),
    };
    w.check_pool(&pool)
        .map_err(|why| (WatchExit::NotPool, why))?;
    w.start_trace(&pool.users).map_err(|error| match error {
        TraceError::ProviderFull(why) => (
            WatchExit::ProviderFull,
            format!("the file provider has no free session: {why}"),
        ),
        TraceError::Other(why) => (WatchExit::NoTrace, why),
    })?;
    if let Err(why) = w.open_pipe(&pool.owner) {
        w.close_pipe();
        w.stop_trace();
        return Err((WatchExit::NoPipe, why));
    }
    let served = serve(w, &pool);
    w.close_pipe();
    w.stop_trace();
    served
}

/// Step 1: LOCAL SYSTEM, from the admin copy.
fn allowed(w: &mut impl Watcher) -> Result<(), Refusal> {
    let refuse = |why: String| (WatchExit::NotAllowed, why);
    if !w.is_system().map_err(refuse)? {
        return Err(refuse("it runs only as LOCAL SYSTEM".to_owned()));
    }
    let exe = w.current_exe().map_err(refuse)?;
    let admin = w.admin_copy().map_err(refuse)?;
    if !same_path(&exe, &admin) {
        return Err(refuse("it runs only from the admin copy".to_owned()));
    }
    Ok(())
}
