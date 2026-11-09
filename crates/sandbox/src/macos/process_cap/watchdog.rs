//! The watchdog: one thread per launched agent, which counts the processes of the agent's
//! session and stops them at [`AGENT_PROCESS_CAP`]. It runs in this process, which outlives the
//! agent, and goes on after the agent exits while anything of its session runs (a `bomb & exit`
//! is still counted until the handle's tree kill ends it); it ends once the session is empty,
//! or when it can no longer count, after it ended the agent ([`Watch::fail`]).
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
    descendants, hand_over_to_session_sweep, verdict,
};

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
#[derive(Debug)]
pub(crate) struct Watch {
    root: Root,
    cap: ProcessCapSession,
    polls: u32,
    /// Where a trip's leftovers go: [`hand_over_to_session_sweep`]; a test's own in tests.
    hand_over: fn(u32, &[Proc]),
}

/// Starts the watchdog of the agent at `pid`, just started; its trips and notices count in
/// `cap`. The first count runs before this returns. An error when the agent's start cannot be
/// read or the thread cannot be made: the caller ends the agent rather than run it uncounted.
pub(crate) fn watch(pid: u32, cap: ProcessCapSession) -> io::Result<()> {
    let mut watch = Watch::start(&Libproc, pid, cap).map_err(io::Error::other)?;
    if watch.step(&Libproc) == Step::Done {
        return Ok(());
    }
    thread::Builder::new()
        .name("clauseo-process-cap".to_owned())
        .spawn(move || watch.run(&Libproc))?;
    Ok(())
}

impl Watch {
    /// The watch of agent `pid`, read from `system`. Its start must be readable: without it no
    /// descendant can be told from a reused pid, so a missing one is logged, told and refused.
    pub(crate) fn start(
        system: &impl System,
        pid: u32,
        cap: ProcessCapSession,
    ) -> Result<Watch, &'static str> {
        let Some(agent) = system.info(pid) else {
            let reason = "the agent's start could not be read";
            tracing::error!(
                agent = pid,
                reason,
                "the agent's process cap could not start"
            );
            cap.tell(ProcessCapNotice::CapFailed);
            return Err(reason);
        };
        Ok(Watch {
            root: Root {
                pid,
                start: agent.start,
            },
            cap,
            polls: 0,
            hand_over: hand_over_to_session_sweep,
        })
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
    pub(crate) fn step(&mut self, system: &impl System) -> Step {
        self.polls = self.polls.wrapping_add(1);
        let Some(members) = members(system, self.root.pid) else {
            return self.fail(system, "the process listing failed");
        };
        if members.is_empty() {
            return Step::Done;
        }
        // While the session lives no other process has the agent's pid, so a member with it is
        // the agent.
        let agent = members.contains(&self.root.pid);
        if agent && !self.agent_readable(system) {
            return self.fail(system, "the agent could not be read");
        }
        let count = members.len() - usize::from(agent);
        let sweep = self.polls.is_multiple_of(SWEEP_EVERY);
        if count <= AGENT_PROCESS_CAP && !sweep {
            return Step::Watch;
        }
        let Some(found) = self.read(system) else {
            return self.fail(system, "the process listing failed");
        };
        if found.len() > AGENT_PROCESS_CAP {
            self.trip(system, agent);
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

    /// The agent's running descendants; `None` when the listing failed.
    fn read(&self, system: &impl System) -> Option<Vec<Proc>> {
        let all: Vec<Proc> = system
            .pids()?
            .into_iter()
            .filter_map(|pid| system.info(pid))
            .collect();
        Some(descendants(self.root.pid, self.root.start, &all))
    }

    /// Stops and kills every descendant of the agent, and the agent too when [`verdict`] says
    /// so; else the agent goes on. `agent`: whether the agent still runs.
    ///
    /// One `killpg(SIGSTOP)` freezes the agent's group, the agent with it. Each listed
    /// descendant is then stopped on its own (those outside the group), until a listing finds
    /// none new; each one stopped is killed, and so is anything a listing still finds. Only then
    /// does the agent get `SIGCONT` (or `SIGKILL`): it was stopped for the trip only. Every
    /// signal but the group's goes to a pid whose start still matches. The group's id is the
    /// agent's pid: the step saw the agent's session alive, and while it lives xnu gives no
    /// process that pid, so no group of that id is outside the session.
    pub(crate) fn trip(&self, system: &impl System, agent: bool) -> Trip {
        let number = self.cap.trip();
        let root = self.root;
        system.signal_group(root.pid, libc::SIGSTOP);
        let mut stopped: Vec<Proc> = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..STOP_ROUNDS {
            let Some(found) = self.read(system) else {
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
            left = self.read(system).unwrap_or_default();
            if left.is_empty() {
                break;
            }
            for process in &left {
                system.signal(process.pid, process.start, libc::SIGKILL);
            }
            system.pause(KILL_PAUSE);
        }
        if !left.is_empty() {
            (self.hand_over)(root.pid, &left);
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

    /// Fails closed: the watchdog can no longer count, so it logs `reason`, kills the agent's
    /// group, the agent and every member of its session it can still list (each by its start),
    /// tells the lawyer, and stops. The group and the members are the agent's for the reason
    /// [`Watch::trip`] gives.
    fn fail(&self, system: &impl System, reason: &'static str) -> Step {
        let root = self.root;
        tracing::error!(
            agent = root.pid,
            reason,
            "the agent's process cap lost count: it ends the agent and its session"
        );
        system.signal_group(root.pid, libc::SIGKILL);
        system.signal(root.pid, root.start, libc::SIGKILL);
        for pid in members(system, root.pid).unwrap_or_default() {
            if let Some(process) = system.info(pid).filter(|p| p.start >= root.start) {
                system.signal(pid, process.start, libc::SIGKILL);
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
