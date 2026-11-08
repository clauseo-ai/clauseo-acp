//! One process's kill, and what the sweep keeps of the processes it ended.
//!
//! **A kill is a signalled handle.** A process is "killed" only once its handle (opened with
//! `SYNCHRONIZE`) is signalled within [`KILL_WAIT`] of the kill: the call's success says
//! nothing about the process being gone (one frozen, or held in the kernel, stays). One still
//! running then is a survivor ([`SweepError::Survived`]), an escapee like one that can't be
//! ended: never logged as killed, asked again (without the wait) when a later round lists it, and
//! the sweep ends in [`SweepError::Left`] with it counted as unkillable, so the slot is
//! quarantined (`RunExit::Escapee`).
//!
//! **Pid and creation time.** A later round's listing of a process the sweep already ended (its
//! pid and creation time, its handle held) is that process's object, not a new one: it is neither
//! killed again nor counted as a round's kill, so it can't keep the rounds going. A listed process
//! whose handle is already signalled before any kill (an exited process another process still
//! holds open, such as WER's zero-thread clone of a crashed process, which that process holds
//! until the job goes) has nothing left to end: it is logged once a sweep, and isn't a kill
//! either. Being listed never says a process lives; only its handle says whether it ended. A
//! clone whose handle isn't signalled is killed and waited on like any other process, and is a
//! survivor if it stays. A process with a survivor's pid but
//! another creation time is another process, decided on afresh.
//!
//! **Jobs.** The processes are found by their token's user over the whole machine, never by job:
//! one in a job nested in this launch's, or in a job of its own, is found all the same, and
//! `TerminateProcess` ends it whatever job holds it.

use std::time::Duration;

use super::refused::Refused;
use super::{KILL_WAIT, Point, Processes, Rule, Seen, Spared, SweepError, failed, spared};

/// The processes a sweep killed, held open until the sweep is over: while a handle to a process
/// is open its pid is never given to another, so a later process whose parent pid names one of
/// them, created after it was, is its child.
///
/// Beside them, the survivors (asked to end, still running) and the exited processes found
/// still listed, so a later round or pass knows each by its pid and creation time.
pub(crate) struct Killed<T> {
    /// Each one's pid, creation time (when the sweep could read it) and open process.
    pub(super) entries: Vec<(u32, Option<u64>, T)>,
    /// The survivors: asked to end, their handle not signalled after [`KILL_WAIT`]. Each one's
    /// pid, creation time, image name and open process.
    survivors: Vec<(u32, Option<u64>, String, T)>,
    /// The exited processes found still listed (held open by another process), each logged
    /// once: pid and creation time.
    exited: Vec<(u32, Option<u64>)>,
}

impl<T> Killed<T> {
    pub(crate) fn new() -> Killed<T> {
        Killed {
            entries: Vec::new(),
            survivors: Vec::new(),
            exited: Vec::new(),
        }
    }

    /// Whether the process `pid` created at `created` is one this sweep ended: same pid, same
    /// creation time. Its handle is held, so its pid is no other process's meanwhile; a creation
    /// time that differs is another process all the same.
    fn holds(&self, pid: u32, created: Option<u64>) -> bool {
        self.entries
            .iter()
            .any(|(known, at, _)| *known == pid && *at == created)
    }

    /// The index of the survivor that is the process `pid` created at `created`.
    fn survivor(&self, pid: u32, created: Option<u64>) -> Option<usize> {
        self.survivors
            .iter()
            .position(|(known, at, _, _)| *known == pid && *at == created)
    }

    /// Their pids, in the order they were killed.
    pub(crate) fn pids(&self) -> Vec<u32> {
        self.entries.iter().map(|(pid, _, _)| *pid).collect()
    }

    /// Whether a process whose parent pid is `parent`, created at `started`, was started by one
    /// of them: one has that pid and was created at or before it.
    pub(super) fn started_by(&self, parent: Option<u32>, started: u64) -> bool {
        parent.is_some_and(|parent| {
            self.entries.iter().any(|(pid, created, _)| {
                *pid == parent && created.is_some_and(|created| created <= started)
            })
        })
    }
}

/// What [`one`] came to.
pub(super) enum Done<'a, T> {
    /// Nothing: kept, already ended, or exited and held open by another process.
    Nothing,
    /// Its handle was signalled after its kill (logged): a round's kill.
    Killed,
    /// Its kill was refused: looked at again after the round's kills.
    Refused(Refused<'a, T>),
    /// It couldn't be checked, or survived its kill.
    Failed(SweepError),
}

/// Decides on the process `found`, opened as `process` and listed again for the pool user, and
/// kills it unless `rule` or `killed` keeps it; a kill and an exited process are lines given to
/// `log`.
pub(super) fn one<'a, P: Processes>(
    calls: &mut P,
    (found, process): (&'a Seen, P::Process),
    rule: &Rule,
    killed: &mut Killed<P::Process>,
    log: &mut dyn FnMut(&str),
) -> Done<'a, P::Process> {
    let point = rule.point;
    // Its identity beside its pid; one whose time can't be read is known by its pid alone,
    // which the held handles pin.
    let created = calls.started(&process);
    let known = created.as_ref().ok().copied();
    // The object of a process this sweep already ended: not a new kill.
    if killed.holds(found.pid, known) {
        return Done::Nothing;
    }
    if let Some(index) = killed.survivor(found.pid, known) {
        return match retried(calls, killed, index) {
            Some(ended) => {
                log(&killed_line(point, found));
                killed.entries.push(ended);
                Done::Killed
            }
            None => Done::Failed(survived(found)),
        };
    }
    let started = match spared(found, created, rule, killed) {
        Ok(Spared::Yes) => return Done::Nothing,
        Ok(Spared::No(started)) => started.or(known),
        Err((error, step)) => return Done::Failed(failed(found, &error, step)),
    };
    // Exited already, still listed: another process holds it open. Nothing to end.
    if calls.exited(&process, Duration::ZERO) {
        if !killed.exited.contains(&(found.pid, known)) {
            killed.exited.push((found.pid, known));
            log(&format!(
                "clauseo-sandbox-run: sweep ({point}): the sandbox user's process {} ({}) had \
                 already exited (another process holds it open)",
                found.pid, found.image
            ));
        }
        return Done::Nothing;
    }
    if let Err(error) = calls.kill(&process) {
        return Done::Refused(Refused {
            found,
            process,
            started,
            error,
        });
    }
    if !calls.exited(&process, KILL_WAIT) {
        killed
            .survivors
            .push((found.pid, started, found.image.clone(), process));
        return Done::Failed(survived(found));
    }
    log(&killed_line(point, found));
    killed.entries.push((found.pid, started, process));
    Done::Killed
}

fn survived(found: &Seen) -> SweepError {
    SweepError::Survived {
        pid: found.pid,
        image: found.image.clone(),
    }
}

/// The line logged for a process whose handle was signalled after its kill.
fn killed_line(point: Point, found: &Seen) -> String {
    format!(
        "clauseo-sandbox-run: sweep ({point}) killed the sandbox user's process {} ({})",
        found.pid, found.image
    )
}

/// Asks the survivor at `index` to end again, without waiting: its entry for
/// [`Killed::entries`] once its handle is signalled, else `None` (it stays a survivor).
fn retried<P: Processes>(
    calls: &mut P,
    killed: &mut Killed<P::Process>,
    index: usize,
) -> Option<(u32, Option<u64>, P::Process)> {
    let process = &killed.survivors.get(index)?.3;
    // Its success says nothing; the handle does.
    drop(calls.kill(process));
    if !calls.exited(process, Duration::ZERO) {
        return None;
    }
    let (pid, created, _, process) = killed.survivors.swap_remove(index);
    Some((pid, created, process))
}
