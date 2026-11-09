//! The watchdog: one thread per launched agent, which counts the processes of the agent's
//! session and stops them at [`AGENT_PROCESS_CAP`]. It runs in this process, which outlives the
//! agent, and goes on after the agent exits while anything of its session runs (a `bomb & exit`
//! is still counted until the handle's tree kill ends it), and after the session empties while
//! the launch's mark still finds a process (a `setsid bomb &` and then the agent killed): it
//! ends once neither finds anything or session end ended the mark, or when it can no longer
//! count, after it ended the agent ([`Watch::fail`]).
//!
//! Each [`POLL`] it lists every pid (`proc_listallpids`) and asks each one's session (`getsid`):
//! the members of the agent's session, which job control (`set -m`, `setpgid`) does not leave,
//! orphans included. A zombie has no session to `getsid` (it answers `ESRCH`), so zombies never
//! count. Past the cap, or every [`SWEEP_EVERY`] polls, it reads every process (`proc_pidinfo`)
//! and takes the agent's [`descendants`], by session and by parent; only that count trips the
//! cap. One listing runs per poll, whatever the agent does; a trip is a bounded burst of them.
//!
//! No kqueue: a count by session needs no wake on the agent's forks (the poll's listing sees
//! them, and one listing a poll is the most it runs) nor on its exit (the session outlives it).
//! The first count runs before the launch returns, and sees every process the agent started
//! since its exec, as none of them can have left the session but by `setsid`.

use std::collections::HashSet;
use std::ffi::c_int;
use std::io;
use std::thread;
use std::time::Duration;

use super::libproc::Libproc;
use super::{
    AGENT_PROCESS_CAP, POLL, Proc, ProcessCapNotice, ProcessCapSession, SWEEP_EVERY, Verdict,
    descendants, verdict,
};
use crate::macos::sweep::SweepError;

/// How many times a trip lists the tree again to stop what the last round missed (a child
/// forked while its parent was being stopped, or one outside the group).
const STOP_ROUNDS: usize = 8;
/// How many times a trip lists the tree again to kill what is still running after the kill.
const KILL_ROUNDS: usize = 20;
/// The wait between two kill rounds, while the killed processes exit.
const KILL_PAUSE: Duration = Duration::from_millis(10);

/// What the watchdog reads and signals: this Mac's processes ([`Libproc`]), or a test's.
pub(crate) trait System {
    /// Every pid, zombies included; `None` when the listing fails.
    fn pids(&self) -> Option<Vec<u32>>;
    /// The session of `pid`; `None` when it is gone or a zombie.
    fn session_of(&self, pid: u32) -> Option<u32>;
    /// `pid` as a [`Proc`]; `None` when it is gone, a zombie or unreadable.
    fn info(&self, pid: u32) -> Option<Proc>;
    /// Sends `signal` to `pid` if it still runs with `start`; whether it went.
    fn signal(&self, pid: u32, start: u64, signal: c_int) -> bool;
    /// Sends `signal` to process group `group`; whether it went.
    fn signal_group(&self, group: u32, signal: c_int) -> bool;
    /// Waits `time`.
    fn pause(&self, time: Duration);
}

/// What the watchdog asks the session sweep about its own launch: `sweep::LaunchSweep`, which
/// holds that launch's mark and no other, or a test's.
pub(crate) trait Escapes: Send {
    /// The launch's processes among `rest` (processes the tree walk did not find), the agent
    /// and the launch's sentinel left out; `Ok(None)` once session end ended the launch; an
    /// error when the scan failed (the watchdog then fails closed).
    fn escaped(&self, rest: &[Proc]) -> Result<Option<Vec<Proc>>, SweepError>;
    /// Stops and kills what a trip's kill rounds left (`left`) with every process of the
    /// launch the mark still finds, the agent left out; answers how many still run.
    fn take_over(&self, left: &[Proc]) -> usize;
}

/// The agent: its pid and start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Root {
    pub(crate) pid: u32,
    pub(crate) start: u64,
}

/// What a poll leaves the watchdog to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Poll again.
    Watch,
    /// Stop: the session is empty, or the watchdog ended it.
    Done,
}

/// What a trip did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Trip {
    /// The trip's number in the session, from 1.
    pub(crate) number: u32,
    /// What it did to the agent (spared when the agent had exited already).
    pub(crate) verdict: Verdict,
    /// The descendants it stopped.
    pub(crate) stopped: usize,
    /// What its kill rounds left running, handed to the session sweep.
    pub(crate) left: Vec<Proc>,
}

/// One agent's watch.
pub(crate) struct Watch {
    root: Root,
    cap: ProcessCapSession,
    polls: u32,
    /// The session sweep of this launch: its escapees, and where a trip's leftovers go.
    escapes: Box<dyn Escapes>,
    /// Whether the agent's session has emptied with processes of the launch still found by its
    /// mark: from then on they are read at each sweep.
    orphaned: bool,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch")
            .field("root", &self.root)
            .field("polls", &self.polls)
            .field("orphaned", &self.orphaned)
            .finish_non_exhaustive()
    }
}

/// Starts the watchdog of the agent at `pid`, just started; its trips and notices count in
/// `cap`, and `escapes` is its launch's session sweep. The first count runs before this
/// returns; an agent that has exited already with nothing of its session left needs no watch.
/// An error when a running agent's start cannot be read or the thread cannot be made: the
/// caller ends the agent rather than run it uncounted.
pub(crate) fn watch(pid: u32, cap: ProcessCapSession, escapes: Box<dyn Escapes>) -> io::Result<()> {
    let Some(mut watch) = Watch::start(&Libproc, pid, cap, escapes).map_err(io::Error::other)?
    else {
        return Ok(());
    };
    if watch.step(&Libproc) == Step::Done {
        return Ok(());
    }
    thread::Builder::new()
        .name("clauseo-process-cap".to_owned())
        .spawn(move || watch.run(&Libproc))?;
    Ok(())
}

impl Watch {
    /// The watch of agent `pid`, read from `system`; `None` when there is nothing to watch.
    ///
    /// An agent that exited at once (gone, or a zombie: no session answers for it) is a normal
    /// end. With its session empty there is nothing to watch; with members left, they are
    /// watched by session alone (start 0): while one lives no other process has the agent's
    /// pid, so its session id is the agent's. A running agent whose start cannot be read is a
    /// failure: no descendant could be told from a reused pid, so it is logged, told and
    /// refused.
    pub(crate) fn start(
        system: &impl System,
        pid: u32,
        cap: ProcessCapSession,
        escapes: Box<dyn Escapes>,
    ) -> Result<Option<Watch>, &'static str> {
        let start = match system.info(pid) {
            Some(agent) => agent.start,
            None if system.session_of(pid).is_none() => match members(system, pid) {
                Some(members) if members.is_empty() => return Ok(None),
                Some(_) => 0,
                None => return Err(Watch::refuse(&cap, pid, "the process listing failed")),
            },
            None => {
                return Err(Watch::refuse(
                    &cap,
                    pid,
                    "the agent's start could not be read",
                ));
            }
        };
        Ok(Some(Watch {
            root: Root { pid, start },
            cap,
            polls: 0,
            escapes,
            orphaned: false,
        }))
    }

    /// Logs and tells why the watch of agent `pid` could not start; returns `reason`.
    fn refuse(cap: &ProcessCapSession, pid: u32, reason: &'static str) -> &'static str {
        tracing::error!(
            agent = pid,
            reason,
            "the agent's process cap could not start"
        );
        cap.tell(ProcessCapNotice::CapFailed);
        reason
    }

    /// Polls every [`POLL`] until [`Step::Done`].
    fn run(mut self, system: &impl System) {
        loop {
            system.pause(POLL);
            if self.step(system) == Step::Done {
                return;
            }
        }
    }

    /// One poll: one listing of the session, and on a sweep or past the cap a reading of every
    /// process; a trip when the agent's descendants are past the cap.
    ///
    /// Once the session is empty, the launch's mark alone counts what is left (the agent and
    /// its session are gone, so its pid may name another process and is never signalled as a
    /// group): read at once, then at each sweep, until it finds nothing or session end ended it.
    pub(crate) fn step(&mut self, system: &impl System) -> Step {
        self.polls = self.polls.wrapping_add(1);
        let sweep = self.polls.is_multiple_of(SWEEP_EVERY);
        let Some(members) = members(system, self.root.pid) else {
            return self.fail(system, "the process listing failed", true);
        };
        if members.is_empty() {
            if self.orphaned && !sweep {
                return Step::Watch;
            }
            let found = match self.read(system, false) {
                Ok(found) => found,
                Err(reason) => return self.fail(system, reason, false),
            };
            match found {
                None => return Step::Done,
                Some(found) if found.is_empty() => return Step::Done,
                Some(found) => {
                    self.orphaned = true;
                    if found.len() > AGENT_PROCESS_CAP {
                        self.trip(system, false, false);
                    }
                    return Step::Watch;
                }
            }
        }
        // While the session lives no other process has the agent's pid, so a member with it is
        // the agent.
        let agent = members.contains(&self.root.pid);
        if agent && !self.agent_readable(system) {
            return self.fail(system, "the agent could not be read", true);
        }
        let count = members.len() - usize::from(agent);
        if count <= AGENT_PROCESS_CAP && !sweep {
            return Step::Watch;
        }
        let found = match self.read(system, true) {
            Ok(found) => found.unwrap_or_default(),
            Err(reason) => return self.fail(system, reason, true),
        };
        if found.len() > AGENT_PROCESS_CAP {
            self.trip(system, agent, true);
        }
        Step::Watch
    }

    /// Whether the agent still reads with its start, or has just exited (gone between the
    /// listing and the read). An agent alive and unreadable, or with another start, is not.
    fn agent_readable(&self, system: &impl System) -> bool {
        match system.info(self.root.pid) {
            Some(agent) => agent.start == self.root.start,
            None => system.session_of(self.root.pid).is_none(),
        }
    }

    /// The agent's running descendants: by session and parent while its session lives
    /// (`session`), and those the launch's mark finds among the rest (a `setsid` orphan), only
    /// those once the session is empty. `Ok(None)` when the session is empty and session end
    /// ended the launch; an error when the listing or the session sweep failed.
    fn read(&self, system: &impl System, session: bool) -> Result<Option<Vec<Proc>>, &'static str> {
        let all: Vec<Proc> = system
            .pids()
            .ok_or("the process listing failed")?
            .into_iter()
            .filter_map(|pid| system.info(pid))
            .collect();
        let mut found = if session {
            descendants(self.root.pid, self.root.start, &all)
        } else {
            Vec::new()
        };
        // Only the processes the walk did not find are asked about, so a bomb in the session
        // costs no kernel query at a trip.
        let known: HashSet<u32> = found.iter().map(|p| p.pid).collect();
        let rest: Vec<Proc> = all
            .into_iter()
            .filter(|p| !known.contains(&p.pid))
            .collect();
        match self.escapes.escaped(&rest) {
            Ok(Some(escaped)) => found.extend(escaped),
            Ok(None) if session => {}
            Ok(None) => return Ok(None),
            Err(error) => {
                tracing::error!(agent = self.root.pid, %error, "the process cap's session sweep failed");
                return Err("the session sweep failed");
            }
        }
        Ok(Some(found))
    }

    /// Stops and kills every descendant of the agent, and the agent too when [`verdict`] says
    /// so; else the agent goes on. `agent`: whether the agent still runs; `session`: whether
    /// its session does (else its group is never signalled and only the mark's finds count).
    ///
    /// One `killpg(SIGSTOP)` freezes the agent's group, the agent with it. Each listed
    /// descendant is then stopped on its own (those outside the group), until a listing finds
    /// none new; each one stopped is killed, and so is anything a listing still finds. Only then
    /// does the agent get `SIGCONT` (or `SIGKILL`): it was stopped for the trip only. Every
    /// signal but the group's goes to a pid whose start still matches. The group's id is the
    /// agent's pid: the step saw the agent's session alive, and while it lives xnu gives no
    /// process that pid, so no group of that id is outside the session.
    pub(crate) fn trip(&self, system: &impl System, agent: bool, session: bool) -> Trip {
        let number = self.cap.trip();
        let root = self.root;
        if session {
            system.signal_group(root.pid, libc::SIGSTOP);
        }
        let mut stopped: Vec<Proc> = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..STOP_ROUNDS {
            let Ok(Some(found)) = self.read(system, session) else {
                break;
            };
            let fresh: Vec<Proc> = found
                .into_iter()
                .filter(|p| seen.insert((p.pid, p.start)))
                .collect();
            if fresh.is_empty() {
                break;
            }
            for process in &fresh {
                system.signal(process.pid, process.start, libc::SIGSTOP);
            }
            stopped.extend(fresh);
        }
        let verdict = if agent {
            verdict(number, root.pid, &stopped)
        } else {
            Verdict::SpareAgent
        };
        for process in &stopped {
            system.signal(process.pid, process.start, libc::SIGKILL);
        }
        let mut left = Vec::new();
        for _ in 0..KILL_ROUNDS {
            left = self
                .read(system, session)
                .ok()
                .flatten()
                .unwrap_or_default();
            if left.is_empty() {
                break;
            }
            for process in &left {
                system.signal(process.pid, process.start, libc::SIGKILL);
            }
            system.pause(KILL_PAUSE);
        }
        if !left.is_empty() {
            // The session sweep takes them over, with every process the mark still finds.
            let running = self.escapes.take_over(&left);
            if running > 0 {
                tracing::warn!(
                    agent = root.pid,
                    left = running,
                    "the agent's process cap could not end every descendant; it keeps sweeping"
                );
            }
        }
        if agent {
            let last = match verdict {
                Verdict::KillAgent => libc::SIGKILL,
                Verdict::SpareAgent => libc::SIGCONT,
            };
            system.signal(root.pid, root.start, last);
        }
        self.cap.tell(match verdict {
            Verdict::KillAgent => ProcessCapNotice::AssistantStopped,
            Verdict::SpareAgent => ProcessCapNotice::TooManyPrograms,
        });
        tracing::warn!(
            trip = number,
            count = stopped.len(),
            ?verdict,
            "the agent's process cap stopped its descendants"
        );
        Trip {
            number,
            verdict,
            stopped: stopped.len(),
            left,
        }
    }

    /// Fails closed: the watchdog can no longer count (a listing, the agent's read or the
    /// session sweep failed), so it logs `reason`, kills the agent's group, the agent and every
    /// member of its session it can still list (each by its start), tells the lawyer
    /// ([`ProcessCapNotice::CapFailed`]), and stops. The group and the members are the agent's
    /// for the reason [`Watch::trip`] gives, while its session lives (`session`); once it is
    /// empty, only the agent is signalled, by its start.
    fn fail(&self, system: &impl System, reason: &'static str, session: bool) -> Step {
        let root = self.root;
        tracing::error!(
            agent = root.pid,
            reason,
            "the agent's process cap lost count: it ends the agent and its session"
        );
        if session {
            system.signal_group(root.pid, libc::SIGKILL);
        }
        system.signal(root.pid, root.start, libc::SIGKILL);
        if session {
            for pid in members(system, root.pid).unwrap_or_default() {
                if let Some(process) = system.info(pid).filter(|p| p.start >= root.start) {
                    system.signal(pid, process.start, libc::SIGKILL);
                }
            }
        }
        self.cap.tell(ProcessCapNotice::CapFailed);
        Step::Done
    }
}

/// The pids in session `session`, zombies left out; `None` when the listing failed.
fn members(system: &impl System, session: u32) -> Option<Vec<u32>> {
    Some(
        system
            .pids()?
            .into_iter()
            .filter(|&pid| system.session_of(pid) == Some(session))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
