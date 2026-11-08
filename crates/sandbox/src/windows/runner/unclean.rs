//! What the unclean-session markers (`marker.rs`) hold up, and who settles them. Three callers,
//! each as the lawyer, none holding the set-up lock (only an administrator's token can open it;
//! `setup::pool_steps::sequence` says why reading the numbers without it is sound):
//!
//! - **The pool user's logon** (F4, [`logon_check`], the outer before it writes its own marker):
//!   refused while a marker of this pool user, or one naming none or unreadable, awaits a stop
//!   with `StopSequence` below its number. An `Open` marker of this pool user is a dead outer's
//!   (one live session per pool user, and this outer hasn't written its own yet): it becomes
//!   `AwaitingStop`, with `StopNext` and `SweepNext` read now, after its session's last launch
//!   path closed. Each refusal requests the stop task. TODO: once slot groups land (two pool
//!   users share a slot's grants on a matter), the scope becomes the pool user's slot, since an
//!   escapee of one user still holds the slot group's grants.
//! - **A launch** ([`await_sweeps`], `clean_close::check_launch`, inside the host's 180 s): any
//!   marker awaiting a sweep is clean once `SweepSequence` reaches its number. Otherwise the
//!   launch requests a sweep and waits for it, up to [`SWEEP_WAIT`], and refuses if it still
//!   isn't done. `Open` markers are left alone there: they may be live sessions'.
//! - **The desktop's start** ([`start_up`], once, before any launch): every outer of the last run
//!   is gone, so each `Open` or unreadable marker becomes `AwaitingStop`, and the stop and sweep
//!   tasks are requested for what is still awaited. It never waits.
//!
//! A clean marker is removed by whoever finds it clean. A marker naming a pool user the record no
//! longer lists is stale (a removal stopped every process of that SID before it deleted the
//! account) and is removed too. An absent `…Sequence` counts as 0, an absent `…Next` as 1.
//!
//! No Windows call here: [`Markers`] carries every effect (`unclean/os.rs`), so the tests run on
//! every OS.

use std::io;
use std::time::Duration;

use super::marker::{MarkerState, Sequences, UncleanSession};
use crate::windows::setup::record_values::{STOP_NEXT, SWEEP_NEXT};
use crate::windows::sids::Sid;

#[cfg(windows)]
pub(crate) mod os;
#[cfg(test)]
mod tests;

/// How long a launch waits for a sweep a marker awaits: well inside the host's 180 s.
pub const SWEEP_WAIT: Duration = Duration::from_secs(15);
/// How often it reads `SweepSequence` meanwhile.
pub const SWEEP_POLL: Duration = Duration::from_millis(100);

/// The markers' effects, as the lawyer.
pub trait Markers {
    /// Every marker in the run folder (`marker::unclean_sessions`).
    fn sessions(&mut self) -> io::Result<Vec<UncleanSession>>;
    /// `StopSequence` and `SweepSequence`, each 0 when absent.
    fn sequences(&mut self) -> Result<Sequences, String>;
    /// `name` (`StopNext`, `SweepNext`), 1 when absent.
    fn next_number(&mut self, name: &'static str) -> Result<u64, String>;
    /// The record's `PoolSids`; empty when there is no record or it can't be read, which
    /// makes no marker stale.
    fn pool(&mut self) -> Vec<Sid>;
    /// Writes the marker `id` as `state`, naming `pool_user`.
    fn settle(&mut self, id: &str, pool_user: Option<&Sid>, state: MarkerState) -> io::Result<()>;
    /// Removes the marker `id`.
    fn remove(&mut self, id: &str) -> io::Result<()>;
    /// Asks for a run of `\Clauseo\SandboxStop`: whether it was queued.
    fn request_stop(&mut self) -> bool;
    /// Asks for a run of `\Clauseo\SandboxSweep`.
    fn request_sweep(&mut self) -> Result<(), String>;
    /// Waits `pause`.
    fn pause(&mut self, pause: Duration);
}

/// The markers, the stale ones removed.
fn current(fs: &mut impl Markers) -> io::Result<Vec<UncleanSession>> {
    let sessions = fs.sessions()?;
    let pool = fs.pool();
    let mut kept = Vec::new();
    for session in sessions {
        let stale = !pool.is_empty()
            && session
                .pool_user
                .as_ref()
                .is_some_and(|user| !pool.contains(user));
        if !(stale && fs.remove(&session.id).is_ok()) {
            kept.push(session);
        }
    }
    Ok(kept)
}

/// Removes each of `sessions` that `done` meets; the ones left.
fn sweep_clean(
    fs: &mut impl Markers,
    sessions: Vec<UncleanSession>,
    done: Sequences,
) -> Vec<UncleanSession> {
    let mut left = Vec::new();
    for session in sessions {
        let clean = session.state.is_some_and(|state| state.clean(done));
        if !(clean && fs.remove(&session.id).is_ok()) {
            left.push(session);
        }
    }
    left
}

/// F4: whether `pool_user` may log on. `Err` says why not; the stop task was requested then.
pub fn logon_check(fs: &mut impl Markers, pool_user: &Sid) -> Result<(), String> {
    let sessions = current(fs).map_err(|e| format!("the session markers can't be read ({e})"))?;
    let ours: Vec<UncleanSession> = sessions
        .into_iter()
        .filter(|session| {
            session
                .pool_user
                .as_ref()
                .is_none_or(|user| user == pool_user)
        })
        .collect();
    let mut standing = false;
    let mut awaiting = Vec::new();
    for session in ours {
        match session.state {
            // Its outer died: it now awaits a stop that begins after this read.
            Some(MarkerState::Open) if session.pool_user.is_some() => {
                standing = true;
                if let Some(state) = dead_outer(fs) {
                    drop(fs.settle(&session.id, Some(pool_user), state));
                }
            }
            Some(state @ MarkerState::AwaitingStop { .. }) => awaiting.push((session, state)),
            Some(MarkerState::AwaitingSweep { .. }) => {}
            Some(MarkerState::Open) | None => standing = true,
        }
    }
    if !awaiting.is_empty() {
        match fs.sequences() {
            Ok(done) => {
                for (session, state) in awaiting {
                    if state.stop().is_some_and(|stop| done.stop < stop) {
                        standing = true;
                    } else if state.clean(done) {
                        drop(fs.remove(&session.id));
                    }
                }
            }
            Err(_) => standing = true,
        }
    }
    if !standing {
        return Ok(());
    }
    let asked = if fs.request_stop() {
        "the stop was requested"
    } else {
        "the stop couldn't be requested"
    };
    Err(format!(
        "an earlier session of this sandbox user awaits the sandbox's stop ({asked})"
    ))
}

/// A dead outer's marker: `StopNext` and `SweepNext` read now; `None` when either can't be.
fn dead_outer(fs: &mut impl Markers) -> Option<MarkerState> {
    let stop = fs.next_number(STOP_NEXT).ok()?;
    let sweep = fs.next_number(SWEEP_NEXT).ok()?;
    Some(MarkerState::AwaitingStop { stop, sweep })
}

/// A launch's check: every marker awaiting a sweep is clean, or becomes clean within
/// [`SWEEP_WAIT`] of a sweep this asks for. `Err` says why not.
pub fn await_sweeps(fs: &mut impl Markers) -> Result<(), String> {
    let sessions = current(fs).map_err(|e| format!("the session markers can't be read ({e})"))?;
    let unread = |e: String| format!("the sandbox's sweep sequence can't be read ({e})");
    let mut done = fs.sequences().map_err(unread)?;
    let left = sweep_clean(fs, sessions, done);
    let wanted = left
        .iter()
        .filter_map(|session| session.state.and_then(MarkerState::sweep))
        .max();
    let Some(wanted) = wanted.filter(|wanted| done.sweep < *wanted) else {
        return Ok(());
    };
    // A request that fails may still find a run queued: the wait says.
    drop(fs.request_sweep());
    let polls = SWEEP_WAIT.as_millis() / SWEEP_POLL.as_millis().max(1);
    for _ in 0..polls {
        fs.pause(SWEEP_POLL);
        done = fs.sequences().map_err(unread)?;
        if done.sweep >= wanted {
            sweep_clean(fs, left, done);
            return Ok(());
        }
    }
    Err(format!(
        "an earlier session's sandbox sweep didn't finish within {} s",
        SWEEP_WAIT.as_secs()
    ))
}

/// What the desktop's start found and asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartUp {
    /// Markers still standing once the clean and stale ones went.
    pub left: usize,
    /// Whether a stop was asked for, and queued.
    pub stop_requested: bool,
    /// Whether a sweep was asked for, and queued.
    pub sweep_requested: bool,
}

/// The desktop's start, once, before any launch: every `Open` or unreadable marker is a dead
/// outer's and becomes `AwaitingStop` with the numbers read now; the clean and stale ones are
/// removed; the stop task is requested when a marker still awaits a stop, the sweep task when one
/// still awaits a sweep. It never waits.
pub fn start_up(fs: &mut impl Markers) -> io::Result<StartUp> {
    let mut sessions = current(fs)?;
    let mut numbers = None;
    for session in &mut sessions {
        if session
            .state
            .is_some_and(|state| state != MarkerState::Open)
        {
            continue;
        }
        if numbers.is_none() {
            numbers = dead_outer(fs);
        }
        let Some(state) = numbers else { continue };
        if fs
            .settle(&session.id, session.pool_user.as_ref(), state)
            .is_ok()
        {
            session.state = Some(state);
        }
    }
    let Ok(done) = fs.sequences() else {
        let left = sessions.len();
        return Ok(StartUp {
            left,
            ..StartUp::default()
        });
    };
    let left = sweep_clean(fs, sessions, done);
    let states = || left.iter().filter_map(|session| session.state);
    let stop = states().any(|state| state.stop().is_some_and(|stop| done.stop < stop));
    let sweep = states().any(|state| state.sweep().is_some_and(|sweep| done.sweep < sweep));
    Ok(StartUp {
        left: left.len(),
        stop_requested: stop && fs.request_stop(),
        sweep_requested: sweep && fs.request_sweep().is_ok(),
    })
}
