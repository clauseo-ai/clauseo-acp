//! What the unclean-session markers (`marker.rs`) hold up, and who settles them. Three callers,
//! each as the lawyer, none holding the set-up lock (only an administrator's token can open it;
//! `setup::pool_steps::sequence` says why reading the numbers without it is sound):
//!
//! - **The pool user's logon** (F4, [`logon_check`], the outer before it writes its own marker):
//!   refused while a marker of this pool user, or one naming none or unreadable, awaits a stop
//!   with `StopSequence` below its number. An `Open` marker of this pool user is a dead outer's
//!   (one live session per pool user, and this outer hasn't written its own yet): it becomes
//!   `AwaitingStop`, with `StopNext` read now, after its session's last launch path closed. An
//!   unreadable or corrupt marker, or an `Open` one naming no pool user, becomes the same,
//!   naming none, so it heals once that stop completes instead of refusing every logon for
//!   good. Each such refusal requests the stop task, as does one awaiting a final stop that
//!   hasn't completed. Otherwise a marker of this pool user awaiting a sweep (a re-mark's, or
//!   any other) is waited for, up to [`SWEEP_WAIT`], and refuses the logon if the sweep doesn't
//!   complete, with no stop requested: a task the escapee registered is live until that sweep
//!   ran. One whose sweep completed but owes its final stop is re-marked to await that stop,
//!   which is requested, and the logon refused. TODO: once slot groups land (two pool users
//!   share a slot's grants on a matter), the scope becomes the pool user's slot, since an
//!   escapee of one user still holds the slot group's grants. Before all that, on a per-user
//!   install, a stop or sweep task (`refresh::POOL_TASKS`) that is missing or changed refuses
//!   the logon up front, in the readiness check's words: its session's end couldn't ask for the
//!   sweep, so the launch would run and only the next one would be refused. Tasks that can't be
//!   read are read once more after [`TASK_RETRY`], then refuse it too
//!   ([`TASKS_UNCHECKED_AT_LAUNCH`]). The refresh task never refuses a launch.
//! - **A launch** ([`await_sweeps`], `clean_close::check_launch`, inside the host's 180 s): any
//!   marker awaiting a sweep is clean once `SweepSequence` reaches its number. Otherwise the
//!   launch requests a sweep and waits for it, up to [`SWEEP_WAIT`], and refuses if it still
//!   isn't done. `Open` markers are left alone there: they may be live sessions'. So is one
//!   awaiting a stop, first or final, or whose sweep completed and owes its final stop: F4's.
//!   This refusal and F4's for a sweep say the sweep task is missing, changed or unreadable, in
//!   the readiness check's words (`refresh::task_identity`), rather than that the sweep didn't
//!   finish, when it is.
//! - **The desktop's start** ([`start_up`], once, before any launch): every outer of the last run
//!   is gone, so each `Open` or unreadable marker becomes `AwaitingStop`, and the stop and sweep
//!   tasks are requested for what is still awaited. It never waits.
//!
//! Each of the three re-marks a marker awaiting a stop that completed ([`remark`]): only then,
//! with the escapee dead, it reads `SweepNext` (`M`), requests the sweep and makes the marker
//! `AwaitingSweepBeforeStop { sweep: M }`. A sweep that completed before the stop proves
//! nothing, since the escapee could have registered a task after it. Once that sweep completed,
//! F4 and the desktop's start re-mark it again ([`remark_swept`]): `StopNext` (`s`) read only
//! then, the stop requested, and the marker `AwaitingFinalStop { stop: s }`, clean once
//! `StopSequence >= s`. Until the sweep deleted the pool's tasks and cancelled its BITS jobs, a
//! trigger or a notify command could start a process of the pool user's after the first stop's
//! snapshot, and the sweep ends no process but a task's instance: so stop, sweep, stop. A launch
//! refuses while a marker whose stop completed couldn't be re-marked. A clean marker is removed
//! by whoever finds it clean.
//!
//! A marker naming a pool user the record no longer lists is stale (a removal stopped every
//! process of that SID before it deleted the account) and is removed too. So is a readable
//! marker whose install id isn't the record's `InstallId` (or has one while the record has
//! none): it belongs to a removed install, whose counters its numbers count against. With
//! neither holding one (an install from before the id) nothing changes; an id that can't be
//! read makes no marker stale. An absent `…Sequence` counts as 0, an absent `…Next` as 1.
//!
//! No Windows call here: [`Markers`] carries every effect (`unclean/os.rs`), so the tests run on
//! every OS.

use std::io;
use std::time::Duration;

use super::RunExit;
use super::marker::{MarkerState, Sequences, UncleanSession};
use crate::windows::setup::record_values::{STOP_NEXT, SWEEP_NEXT};
use crate::windows::sids::Sid;
use launch_tasks::tasks_ready;
pub use launch_tasks::{
    HEAL_POLL, HEAL_WAIT, HEALING, TASK_RETRY, TASKS_UNCHECKED_AT_LAUNCH, heal_first,
};

mod launch_tasks;
#[cfg(windows)]
pub(crate) mod os;
#[cfg(test)]
mod tests;

/// How long a launch waits for a sweep a marker awaits: well inside the host's 180 s.
pub const SWEEP_WAIT: Duration = Duration::from_secs(15);
/// How often it reads `SweepSequence` meanwhile.
pub const SWEEP_POLL: Duration = Duration::from_millis(100);

/// Why a marker whose stop completed but couldn't be re-marked refuses a launch and a logon.
const STUCK: &str = "an earlier session's marker couldn't be moved on from its completed stop";
/// Why a marker whose sweep before its final stop completed but couldn't be re-marked refuses
/// a logon.
const STUCK_SWEPT: &str =
    "an earlier session's marker couldn't be moved on from its completed sweep to its final stop";

/// Why F4 refused a logon, with the outer's exit for it: [`RunExit::AwaitingStop`] when it
/// requested the stop, [`RunExit::AwaitingSweep`] when no stop is awaited (a sweep that didn't
/// complete, or a marker whose completed stop couldn't be re-marked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The outer's exit.
    pub exit: RunExit,
    /// What it says on stderr.
    pub why: String,
}

impl Refused {
    fn stop(why: String) -> Self {
        Refused {
            exit: RunExit::AwaitingStop,
            why,
        }
    }

    fn sweep(why: String) -> Self {
        Refused {
            exit: RunExit::AwaitingSweep,
            why,
        }
    }
}

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
    /// The record's `InstallId`: `None` when it has none; `Err` when it can't be read, which
    /// makes no marker stale.
    fn install_id(&mut self) -> Result<Option<String>, String>;
    /// Writes the marker `id` as `state`, naming `pool_user`, under the install id `install`.
    fn settle(
        &mut self,
        id: &str,
        pool_user: Option<&Sid>,
        install: Option<&str>,
        state: MarkerState,
    ) -> io::Result<()>;
    /// Removes the marker `id`.
    fn remove(&mut self, id: &str) -> io::Result<()>;
    /// Asks for a run of `\Clauseo\SandboxStop`: whether it was queued.
    fn request_stop(&mut self) -> bool;
    /// Asks for a run of `\Clauseo\SandboxSweep`.
    fn request_sweep(&mut self) -> Result<(), String>;
    /// Waits `pause`.
    fn pause(&mut self, pause: Duration);
    /// Says `line` on the outer's stderr: what went wrong but refuses nothing.
    fn note(&mut self, line: &str);
    /// What the app's repair would mend in a per-user install's stop and sweep tasks
    /// (`refresh::POOL_TASKS`; never the refresh task), in the readiness check's words
    /// (`refresh::tasks_repair`: missing, changed, or unreadable); `None` while each is the
    /// set-up's, or on a per-machine install.
    fn task_repair(&mut self) -> Option<&'static str>;
    /// Asks for a run of the refresh task, which puts the tasks back as SYSTEM: whether it
    /// was asked for (`refresh::launch_admin_refresh`, bounded).
    fn heal_tasks(&mut self) -> bool;
    /// The record's `AdminCopySha256` as the lawyer reads it, `None` when it can't be read: it
    /// changes when a refresh run swaps a new admin copy in.
    fn admin_copy_stamp(&mut self) -> Option<String>;
}

/// Why a sweep a marker awaits didn't complete: `waited`, unless a task is missing, changed
/// or can't be read, which no wait mends: then the readiness check's words for it.
fn unswept(fs: &mut impl Markers, waited: String) -> String {
    fs.task_repair().map_or(waited, str::to_owned)
}

/// The markers, the stale ones removed: those of a pool user the record no longer lists, and
/// those of another install. An unreadable one is given the current install id, if known, for
/// when it is written again. With them, whether the id was read: until it is, no unreadable
/// marker is written again, so none is written id-less and then removed as another install's.
fn current(fs: &mut impl Markers) -> io::Result<(Vec<UncleanSession>, bool)> {
    let sessions = fs.sessions()?;
    let pool = fs.pool();
    let install = fs.install_id().ok();
    let mut kept = Vec::new();
    for mut session in sessions {
        let gone_user = !pool.is_empty()
            && session
                .pool_user
                .as_ref()
                .is_some_and(|user| !pool.contains(user));
        let readable = session.state.is_some();
        let other_install = readable && install.as_ref().is_some_and(|now| session.install != *now);
        if (gone_user || other_install) && fs.remove(&session.id).is_ok() {
            continue;
        }
        if !readable && let Some(now) = &install {
            session.install.clone_from(now);
        }
        kept.push(session);
    }
    Ok((kept, install.is_some()))
}

/// Writes `session`'s marker as `state`, as it names its pool user and install.
fn settle(fs: &mut impl Markers, session: &UncleanSession, state: MarkerState) -> io::Result<()> {
    let install = session.install.as_deref();
    fs.settle(&session.id, session.pool_user.as_ref(), install, state)
}

/// Removes each of `sessions` that `done` shows clean ([`MarkerState::clean`]); the ones left.
fn remove_clean(
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

/// Every one of `sessions` awaiting a stop that `done` shows completed, re-marked: `SweepNext`
/// read now (once), the sweep requested, and the marker written as awaiting that sweep before
/// its final stop. One whose number can't be read, or whose write fails, stays as it was. The
/// sessions as they now stand, and whether a sweep was requested (`Some(queued)`).
fn remark(
    fs: &mut impl Markers,
    mut sessions: Vec<UncleanSession>,
    done: Sequences,
) -> (Vec<UncleanSession>, Option<bool>) {
    if !sessions
        .iter()
        .any(|session| session.state.is_some_and(|state| state.stopped(done)))
    {
        return (sessions, None);
    }
    // Read only now that `StopSequence` shows the escapee dead.
    let Ok(sweep) = fs.next_number(SWEEP_NEXT) else {
        return (sessions, None);
    };
    let queued = fs.request_sweep().is_ok();
    let state = MarkerState::AwaitingSweepBeforeStop { sweep };
    for session in &mut sessions {
        if session.state.is_some_and(|state| state.stopped(done))
            && settle(fs, session, state).is_ok()
        {
            session.state = Some(state);
        }
    }
    (sessions, Some(queued))
}

/// Every one of `sessions` awaiting a sweep before its final stop that `done` shows completed,
/// re-marked: `StopNext` read now (once), after that sweep completed, and the marker written
/// as awaiting that final stop. The caller requests the stop. One whose number can't be read,
/// or whose write fails, stays as it was. The sessions as they now stand.
fn remark_swept(
    fs: &mut impl Markers,
    mut sessions: Vec<UncleanSession>,
    done: Sequences,
) -> Vec<UncleanSession> {
    let swept = |session: &UncleanSession| session.state.is_some_and(|state| state.swept(done));
    if !sessions.iter().any(swept) {
        return sessions;
    }
    // Read only now that `SweepSequence` shows the pool's tasks and BITS jobs gone.
    let Ok(stop) = fs.next_number(STOP_NEXT) else {
        return sessions;
    };
    let state = MarkerState::AwaitingFinalStop { stop };
    for session in &mut sessions {
        if swept(session) && settle(fs, session, state).is_ok() {
            session.state = Some(state);
        }
    }
    sessions
}

/// F4: whether `pool_user` may log on. `Err` says why not: first, a SYSTEM task the install
/// needs that is missing or changed, or that can't be read even once more after [`TASK_RETRY`],
/// refuses it before any marker is read ([`Markers::task_repair`], `tasks_ready`). Then the
/// stop task was requested when a marker awaits a stop, first or final; a marker awaiting a
/// sweep (a re-mark's, or any other) is waited for instead, up to [`SWEEP_WAIT`], with no stop
/// requested, since a stop ends every
/// session. One whose sweep before its final stop completed (now or earlier) is re-marked to
/// await that stop, which is requested, and refuses the logon. One whose stop or sweep completed
/// but couldn't be re-marked refuses the logon with no stop requested. One whose final stop
/// completed is clean. A clean one that can't be removed refuses nothing: it is noted
/// ([`Markers::note`]).
pub fn logon_check(fs: &mut impl Markers, pool_user: &Sid) -> Result<(), Refused> {
    tasks_ready(fs)?;
    let (sessions, known) = current(fs)
        .map_err(|e| Refused::stop(format!("the session markers can't be read ({e})")))?;
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
    let mut sweeping = Vec::new();
    let mut stop_next = None;
    for session in ours {
        match session.state {
            // Its outer died, or it can't be read (or names no one): it now awaits a stop that
            // begins after this read, so it heals once that stop completes.
            Some(MarkerState::Open) | None => {
                standing = true;
                if stop_next.is_none() {
                    stop_next = dead_outer(fs);
                }
                // An unreadable one waits for the install id: written without it, it would read
                // as another install's.
                let rewritable = known || session.state.is_some();
                if let Some(state) = stop_next
                    && rewritable
                {
                    drop(settle(fs, &session, state));
                }
            }
            Some(MarkerState::AwaitingStop { .. } | MarkerState::AwaitingFinalStop { .. }) => {
                awaiting.push(session);
            }
            Some(
                MarkerState::AwaitingSweep { .. } | MarkerState::AwaitingSweepBeforeStop { .. },
            ) => sweeping.push(session),
        }
    }
    let mut requested = None;
    let mut stuck = false;
    if !awaiting.is_empty() {
        match fs.sequences() {
            // One whose stop completed is re-marked, and its sweep then waited for below; one
            // whose final stop completed is clean.
            Ok(done) => {
                let (left, asked) = remark(fs, awaiting, done);
                let left = remove_clean(fs, left, done);
                requested = asked;
                standing |= left.iter().any(|session| {
                    session
                        .state
                        .and_then(MarkerState::stop)
                        .is_some_and(|stop| done.stop < stop)
                });
                // Its stop completed but it couldn't be re-marked: no sweep number to wait
                // for, and no stop to ask for, so it refuses as a launch does.
                stuck = left
                    .iter()
                    .any(|session| session.state.is_some_and(|state| state.stopped(done)));
                sweeping.extend(left);
            }
            Err(_) => standing = true,
        }
    }
    if !standing && stuck {
        return Err(Refused::sweep(STUCK.to_owned()));
    }
    let mut awaits = "the sandbox's stop";
    if !standing {
        // A task the escapee registered is live until its sweep ran: wait for it here.
        let (left, done) = wait_for_sweeps(fs, sweeping, requested.is_some())?;
        // Only one awaiting the re-mark to its final stop owes a stop. Any other left is clean
        // but couldn't be removed (another program holds it open): it refuses nothing.
        let (swept, unremoved): (Vec<_>, Vec<_>) = left
            .into_iter()
            .partition(|session| session.state.is_some_and(|state| state.swept(done)));
        for session in unremoved {
            fs.note(&format!(
                "an earlier session's marker {} is clean but couldn't be removed",
                session.id
            ));
        }
        if swept.is_empty() {
            return Ok(());
        }
        // Its sweep completed: what a trigger started before it is the final stop's, whose
        // number is read only now.
        let left = remark_swept(fs, swept, done);
        if left
            .iter()
            .any(|session| session.state.is_some_and(|state| state.swept(done)))
        {
            return Err(Refused::sweep(STUCK_SWEPT.to_owned()));
        }
        awaits = "the sandbox's final stop, after its sweep";
    }
    let asked = if fs.request_stop() {
        "the stop was requested"
    } else {
        "the stop couldn't be requested"
    };
    Err(Refused::stop(format!(
        "an earlier session of this sandbox user awaits {awaits} ({asked})"
    )))
}

/// F4's wait for this pool user's markers awaiting a sweep: those `SweepSequence` meets go;
/// otherwise a sweep is requested (unless `requested` already) and `SweepSequence` read every
/// [`SWEEP_POLL`] for up to [`SWEEP_WAIT`]. `Err` when it is still short, or can't be read:
/// [`RunExit::AwaitingSweep`], since no stop is awaited. `Ok` gives those whose sweep
/// completed but which owe their final stop, with the sequences that showed it.
fn wait_for_sweeps(
    fs: &mut impl Markers,
    sessions: Vec<UncleanSession>,
    requested: bool,
) -> Result<(Vec<UncleanSession>, Sequences), Refused> {
    let sessions: Vec<UncleanSession> = sessions
        .into_iter()
        .filter(|session| session.state.and_then(MarkerState::sweep).is_some())
        .collect();
    if sessions.is_empty() {
        return Ok((sessions, Sequences::default()));
    }
    let unread =
        |e: String| Refused::sweep(format!("the sandbox's sweep sequence can't be read ({e})"));
    let mut done = fs.sequences().map_err(unread)?;
    let left = remove_clean(fs, sessions, done);
    let wanted = left
        .iter()
        .filter_map(|session| session.state.and_then(MarkerState::sweep))
        .max();
    let Some(wanted) = wanted.filter(|wanted| done.sweep < *wanted) else {
        return Ok((left, done));
    };
    if !requested {
        drop(fs.request_sweep());
    }
    let polls = SWEEP_WAIT.as_millis() / SWEEP_POLL.as_millis().max(1);
    for _ in 0..polls {
        fs.pause(SWEEP_POLL);
        done = fs.sequences().map_err(unread)?;
        if done.sweep >= wanted {
            return Ok((remove_clean(fs, left, done), done));
        }
    }
    let waited = format!(
        "an earlier session of this sandbox user awaits the sandbox's sweep, which didn't \
         finish within {} s",
        SWEEP_WAIT.as_secs()
    );
    Err(Refused::sweep(unswept(fs, waited)))
}

/// A dead outer's marker: `StopNext` read now; `None` when it can't be.
fn dead_outer(fs: &mut impl Markers) -> Option<MarkerState> {
    let stop = fs.next_number(STOP_NEXT).ok()?;
    Some(MarkerState::AwaitingStop { stop })
}

/// A launch's check: every marker awaiting a sweep is clean, or becomes clean within
/// [`SWEEP_WAIT`] of a sweep this asks for; one awaiting the sweep before its final stop is
/// waited for the same way, and is F4's once that sweep completed. `Err` says why not.
pub fn await_sweeps(fs: &mut impl Markers) -> Result<(), String> {
    let (sessions, _) =
        current(fs).map_err(|e| format!("the session markers can't be read ({e})"))?;
    let unread = |e: String| format!("the sandbox's sweep sequence can't be read ({e})");
    let mut done = fs.sequences().map_err(unread)?;
    let (sessions, requested) = remark(fs, sessions, done);
    if sessions
        .iter()
        .any(|session| session.state.is_some_and(|state| state.stopped(done)))
    {
        return Err(STUCK.to_owned());
    }
    let left = remove_clean(fs, sessions, done);
    let wanted = left
        .iter()
        .filter_map(|session| session.state.and_then(MarkerState::sweep))
        .max();
    let Some(wanted) = wanted.filter(|wanted| done.sweep < *wanted) else {
        return Ok(());
    };
    // A request that fails may still find a run queued: the wait says. A re-mark asked already.
    if requested.is_none() {
        drop(fs.request_sweep());
    }
    let polls = SWEEP_WAIT.as_millis() / SWEEP_POLL.as_millis().max(1);
    for _ in 0..polls {
        fs.pause(SWEEP_POLL);
        done = fs.sequences().map_err(unread)?;
        if done.sweep >= wanted {
            remove_clean(fs, left, done);
            return Ok(());
        }
    }
    let waited = format!(
        "an earlier session's sandbox sweep didn't finish within {} s",
        SWEEP_WAIT.as_secs()
    );
    Err(unswept(fs, waited))
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
/// outer's and becomes `AwaitingStop` with `StopNext` read now; one whose stop completed is
/// re-marked ([`remark`]), and one whose sweep before its final stop completed too
/// ([`remark_swept`]); the clean and stale ones are removed; the stop task is requested when a
/// marker still awaits a stop, first or final, the sweep task when one still awaits a sweep. It
/// never waits.
pub fn start_up(fs: &mut impl Markers) -> io::Result<StartUp> {
    let (mut sessions, known) = current(fs)?;
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
        let rewritable = known || session.state.is_some();
        if rewritable && settle(fs, session, state).is_ok() {
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
    let (sessions, remarked) = remark(fs, sessions, done);
    let sessions = remark_swept(fs, sessions, done);
    let left = remove_clean(fs, sessions, done);
    let states = || left.iter().filter_map(|session| session.state);
    let stop = states().any(|state| state.stop().is_some_and(|stop| done.stop < stop));
    let sweep = states().any(|state| state.sweep().is_some_and(|sweep| done.sweep < sweep));
    let sweep_requested = match remarked {
        Some(queued) => queued,
        None => sweep && fs.request_sweep().is_ok(),
    };
    Ok(StartUp {
        left: left.len(),
        stop_requested: stop && fs.request_stop(),
        sweep_requested,
    })
}
