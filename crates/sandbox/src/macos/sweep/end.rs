//! Ending a session's processes: [`end_session_processes`] and its two callers, the mode-switch
//! gate ([`switch_to_read_only`]) and session end ([`end_session`]).

use std::collections::HashSet;
use std::thread;
use std::time::{Instant, UNIX_EPOCH};

use super::{
    Checker, END_PAUSE, Gone, ProcId, ReadOnly, SessionMark, Survivors, SweepError, api, now,
    scan_with, start_of,
};
use crate::macos::process_cap::libproc::Libproc;
use crate::macos::process_cap::watchdog::System as _;

/// Ends every process of the sessions `marks` name, the launches' agents included, by
/// [`end_session_processes`]; then, only on [`Gone`], ends each sentinel and removes the
/// marks ([`SessionMark::remove`]). A mark that cannot be removed is logged: the session is
/// gone.
pub fn end_session(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    let gone = end_session_processes(marks, deadline)?;
    for mark in marks {
        if let Err(error) = mark.finish() {
            tracing::warn!(tag = mark.tag(), %error, "a session mark could not be removed");
        }
    }
    Ok(gone)
}

/// The mode-switch gate: ends every process of the matter's sessions (`marks`, one per launch)
/// and answers [`ReadOnly`] only on [`Gone`]. On [`Survivors`] the switch fails closed, an
/// unvouched scan ([`SweepError::Unknown`]) among them.
pub fn switch_to_read_only(
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<ReadOnly, Survivors> {
    match api() {
        Ok(check) => gate_with(check, marks, deadline),
        Err(why) => Err(Survivors {
            left: kill_roots(marks),
            why,
        }),
    }
}

/// [`switch_to_read_only`] through `check`.
pub(super) fn gate_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<ReadOnly, Survivors> {
    end_with(check, marks, deadline).map(|gone| ReadOnly { gone })
}

/// Stops, then kills, every process of the sessions `marks` name, until a scan finds none
/// ([`Gone`]) or `deadline` passes ([`Survivors`], [`SweepError::Deadline`]). An unvouched
/// scan (a killed or unmatched sentinel, the drift check) still ends everything it found, round
/// after round, but once it finds nothing the answer is [`Survivors`] with its doubt, never
/// [`Gone`]. A scan that fails outright (a mark's files unusable, the listing) ends the agents
/// and stops there. A dead sentinel is restarted first ([`SessionMark::revive`]), once more if
/// it dies during the end; the sentinels are never signalled.
///
/// Each launch's agent leads its own session, so one `killpg(SIGSTOP)` freezes its group first.
/// Then each round scans: every process found that is not stopped yet gets `SIGSTOP` (its start
/// checked first), and the round repeats, since a stopped process cannot fork; once a round
/// finds nothing new, each one found gets `SIGKILL`, its start checked again. The scans have no
/// start filter (about 1.3 ms each rather than 0.2): process starts are wall-clock stamps, and a
/// clock set back must not hide a process from the gate.
pub fn end_session_processes(marks: &[&SessionMark], deadline: Instant) -> Result<Gone, Survivors> {
    match api() {
        Ok(check) => end_with(check, marks, deadline),
        Err(why) => Err(Survivors {
            left: kill_roots(marks),
            why,
        }),
    }
}

/// The launches' agents that still run with their starts.
fn live_roots(marks: &[&SessionMark]) -> Vec<ProcId> {
    marks
        .iter()
        .filter_map(|mark| mark.root())
        .filter(|root| start_of(root.pid) == Some(root.start))
        .collect()
}

/// Kills the launches' agents that still run (by their starts); answers them.
fn kill_roots(marks: &[&SessionMark]) -> Vec<ProcId> {
    let left = live_roots(marks);
    for root in &left {
        Libproc.signal(root.pid, root.start, libc::SIGKILL);
    }
    left
}

/// [`end_session_processes`] through `check`.
pub(super) fn end_with(
    check: &dyn Checker,
    marks: &[&SessionMark],
    deadline: Instant,
) -> Result<Gone, Survivors> {
    for root in live_roots(marks) {
        Libproc.signal_group(root.pid, libc::SIGSTOP);
    }
    for mark in marks {
        mark.revive(check);
    }
    let mut retried = false;
    let mut stopped: HashSet<ProcId> = HashSet::new();
    let mut ended: HashSet<ProcId> = HashSet::new();
    loop {
        let scan = match scan_with(check, marks, UNIX_EPOCH) {
            Ok(scan) => scan,
            // Nothing can be found; the agents themselves are known, so they go.
            Err(why) => {
                return Err(Survivors {
                    left: kill_roots(marks),
                    why,
                });
            }
        };
        if !retried && matches!(scan.doubt, Some(SweepError::SentinelGone { .. })) {
            retried = true;
            for mark in marks {
                mark.revive(check);
            }
        }
        let mut targets = scan.found;
        // An agent still starting may not carry its profile yet; it is ended all the same.
        for root in live_roots(marks) {
            if !targets.contains(&root) {
                targets.push(root);
            }
        }
        if targets.is_empty() {
            return match scan.doubt {
                None => Ok(Gone { ended: ended.len() }),
                Some(why) => Err(Survivors {
                    left: Vec::new(),
                    why,
                }),
            };
        }
        if now() >= deadline {
            for id in &targets {
                Libproc.signal(id.pid, id.start, libc::SIGKILL);
            }
            return Err(Survivors {
                left: targets,
                why: SweepError::Deadline,
            });
        }
        let fresh: Vec<ProcId> = targets
            .iter()
            .copied()
            .filter(|id| !stopped.contains(id))
            .collect();
        if fresh.is_empty() {
            for id in &targets {
                if Libproc.signal(id.pid, id.start, libc::SIGKILL) {
                    ended.insert(*id);
                }
            }
            thread::park_timeout(END_PAUSE);
        } else {
            for id in fresh {
                Libproc.signal(id.pid, id.start, libc::SIGSTOP);
                stopped.insert(id);
            }
        }
    }
}
