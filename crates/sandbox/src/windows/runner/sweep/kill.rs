//! One process's kill, and what the sweep keeps of the processes it ended.
//!
//! **A kill is a signalled handle.** A process is "killed" only once its handle (opened with
//! `SYNCHRONIZE`) is signalled within [`KILL_WAIT`] of the kill: the call's success says
//! nothing about the process being gone (one frozen, or held in the kernel, stays). One still
//! running then is a survivor ([`SweepError::Survived`]), an escapee like one that can't be
//! ended: never logged as killed. The verdict is the held handles', not the listings': at each
//! round's end every survivor is asked again (without the wait) and its handle looked at, listed
//! or not ([`settle`]); one still unsignalled is counted as unkillable, so the sweep ends in
//! [`SweepError::Left`]: the inner reports an escapee (`RunExit::Escapee`), and the
//! session's marker awaits the `SYSTEM` stop. Every wait is cut to
//! what is left of the sweep's deadline (`SWEEP_DEADLINE`): past it a kill waits for nothing, and
//! a process not gone at once is a survivor.
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
use super::{Failures, KILL_WAIT, Point, Processes, Rule, Seen, Step, SweepError, failed, spared};

/// A process the sweep holds open: its pid, creation time (when the sweep could read it) and
/// handle.
struct Held<T> {
    pid: u32,
    created: Option<u64>,
    /// Held open, never read: it pins the pid.
    _process: T,
    /// The sweep ended it (killed, or exited by itself once refused); else it had exited
    /// before the sweep came to it.
    ended_here: bool,
}

/// A process asked to end whose handle wasn't signalled, or whose kill was refused and which was
/// still running after the refusal's window.
struct Survivor<T> {
    pid: u32,
    created: Option<u64>,
    image: String,
    process: T,
    /// What it is counted as: [`SweepError::Survived`], or the refused kill's
    /// [`SweepError::Kill`].
    error: SweepError,
    /// Found this round: its kill has just been waited on, and isn't asked again until the next.
    fresh: bool,
}

/// The processes a sweep holds open until it is over, so a pid it gives is never another
/// process's meanwhile: the ones it ended, the exited ones it found still listed, and the
/// survivors (asked to end, still running), so a later round knows each by its pid and creation
/// time.
pub(crate) struct Killed<T> {
    held: Vec<Held<T>>,
    survivors: Vec<Survivor<T>>,
    /// The ones that couldn't be opened ([`SweepError::Open`]): with no handle to look at, each
    /// is counted every round, however it is listed (failing closed), unless a later open of its
    /// pid ends it.
    unopened: Vec<SweepError>,
}

impl<T> Killed<T> {
    pub(crate) fn new() -> Killed<T> {
        Killed {
            held: Vec::new(),
            survivors: Vec::new(),
            unopened: Vec::new(),
        }
    }

    /// Keeps `error`, a process that couldn't be opened, to be counted every round.
    pub(super) fn unopened(&mut self, error: SweepError) {
        if let SweepError::Open { pid, .. } = &error
            && !self
                .unopened
                .iter()
                .any(|known| matches!(known, SweepError::Open { pid: other, .. } if other == pid))
        {
            self.unopened.push(error);
        }
    }

    /// Keeps `process` (`found`, created at `created`), whose kill was refused and which still
    /// runs, as a survivor counted as `error`.
    pub(super) fn survivor(
        &mut self,
        found: &Seen,
        created: Option<u64>,
        process: T,
        error: SweepError,
    ) {
        self.survivors.push(Survivor {
            pid: found.pid,
            created,
            image: found.image.clone(),
            process,
            error,
            fresh: true,
        });
    }

    /// Keeps `process` as one the sweep ended; a process of its pid that couldn't be opened
    /// before isn't counted any more.
    pub(super) fn ended(&mut self, pid: u32, created: Option<u64>, process: T) {
        self.unopened
            .retain(|known| !matches!(known, SweepError::Open { pid: other, .. } if *other == pid));
        self.held.push(Held {
            pid,
            created,
            _process: process,
            ended_here: true,
        });
    }

    /// Whether the process `pid` created at `created` is one this sweep holds (ended, or found
    /// exited): same pid, same creation time. Its handle is held, so its pid is no other
    /// process's meanwhile; a creation time that differs is another process all the same.
    fn holds(&self, pid: u32, created: Option<u64>) -> bool {
        self.held
            .iter()
            .any(|held| held.pid == pid && held.created == created)
    }

    /// Whether the process `pid` created at `created` is a survivor.
    fn survives(&self, pid: u32, created: Option<u64>) -> bool {
        self.survivors
            .iter()
            .any(|survivor| survivor.pid == pid && survivor.created == created)
    }

    /// The pids it ended, in order: killed, or exited by themselves once refused.
    pub(crate) fn pids(&self) -> Vec<u32> {
        self.held
            .iter()
            .filter(|held| held.ended_here)
            .map(|held| held.pid)
            .collect()
    }
}

/// What [`one`] came to.
pub(super) enum Done<'a, T> {
    /// Nothing for the round: kept, already held, a survivor (looked at by [`settle`]), or
    /// exited and held open by another process.
    Nothing,
    /// Its handle was signalled after its kill (logged): a round's kill.
    Killed,
    /// Its kill was refused: looked at again after the round's kills.
    Refused(Refused<'a, T>),
    /// It couldn't be checked.
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
    // A process this sweep holds, or a survivor: [`settle`] looks at its handle.
    if killed.holds(found.pid, known) || killed.survives(found.pid, known) {
        return Done::Nothing;
    }
    match spared(found, created, rule) {
        Ok(true) => return Done::Nothing,
        Ok(false) => {}
        Err(error) => return Done::Failed(failed(found, &error, Step::Started)),
    }
    let started = known;
    // Exited already, still listed: another process holds it open. Nothing to end; held, so it
    // isn't looked at again.
    if calls.exited(&process, Duration::ZERO) {
        log(&format!(
            "clauseo-sandbox-run: sweep ({point}): the sandbox user's process {} ({}) had \
             already exited (another process holds it open)",
            found.pid, found.image
        ));
        killed.held.push(Held {
            pid: found.pid,
            created: started,
            _process: process,
            ended_here: false,
        });
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
    let wait = KILL_WAIT.min(rule.deadline.saturating_sub(calls.now()));
    if !calls.exited(&process, wait) {
        let error = SweepError::Survived {
            pid: found.pid,
            image: found.image.clone(),
        };
        killed.survivor(found, started, process, error);
        return Done::Nothing;
    }
    log(&killed_line(point, found.pid, &found.image));
    killed.ended(found.pid, started, process);
    Done::Killed
}

/// The held handles' verdict on the survivors, at a round's end, listed again or not: one found
/// this round is counted (its kill was just waited on); an earlier one is asked to end again,
/// without waiting, and is killed (logged, moved to the ones ended) if its handle is signalled
/// now, else counted (once a round: [`Failures::add`]). Then each process that couldn't be
/// opened is counted again. How many ended.
pub(super) fn settle<P: Processes>(
    calls: &mut P,
    killed: &mut Killed<P::Process>,
    point: Point,
    failures: &mut Failures,
    log: &mut dyn FnMut(&str),
) -> Result<usize, SweepError> {
    let mut ended = 0;
    let mut index = 0;
    while let Some(survivor) = killed.survivors.get_mut(index) {
        if !survivor.fresh {
            // Its success says nothing; the handle does.
            drop(calls.kill(&survivor.process));
            if calls.exited(&survivor.process, Duration::ZERO) {
                let gone = killed.survivors.swap_remove(index);
                log(&killed_line(point, gone.pid, &gone.image));
                killed.ended(gone.pid, gone.created, gone.process);
                ended += 1;
                continue;
            }
        }
        survivor.fresh = false;
        failures.add(survivor.error.clone(), log)?;
        index += 1;
    }
    for error in &killed.unopened {
        failures.add(error.clone(), log)?;
    }
    Ok(ended)
}

/// The line logged for a process whose handle was signalled after its kill.
fn killed_line(point: Point, pid: u32, image: &str) -> String {
    format!(
        "clauseo-sandbox-run: sweep ({point}) killed the sandbox user's process {pid} ({image})"
    )
}
