//! The watchdog: one thread per launched agent, which counts the agent's descendants and stops
//! them at [`AGENT_PROCESS_CAP`]. It runs in this process, which outlives the agent, and ends
//! when the agent exits (what is left of its group then is the handle's tree kill), or when it
//! killed the agent.
//!
//! The count runs every [`POLL`] and on each `NOTE_FORK` of the agent: the size of the agent's
//! process group (`proc_listpgrppids`, one call). Past the cap, or every [`SWEEP`], it reads
//! every process (`proc_listallpids` and `proc_pidinfo`) and takes the agent's descendants by
//! parent pid and by group ([`descendants`]): a descendant that called `setsid` or `setpgid`
//! is still found by its parent. Only that count trips the cap, so zombies and processes of
//! other groups never do.

use std::collections::HashSet;
use std::io;
use std::thread;
use std::time::{Duration, Instant};

use super::libproc::{self, Queue, Wake};
use super::{
    AGENT_PROCESS_CAP, Notify, POLL, Proc, SWEEP, TOO_MANY_PROGRAMS, Verdict, descendants, verdict,
};

/// How many times a trip lists the tree again to stop what the last round missed (a child
/// forked while its parent was being stopped, or one outside the group).
const STOP_ROUNDS: usize = 8;
/// How many times a trip lists the tree again to kill what is still running after the kill.
const KILL_ROUNDS: usize = 20;
/// The wait between two kill rounds, while the killed processes exit.
const KILL_PAUSE: Duration = Duration::from_millis(10);

/// The agent: its pid and start, read while it was this process's unreaped child.
#[derive(Debug, Clone, Copy)]
struct Root {
    pid: u32,
    start: u64,
}

/// Starts the watchdog of the agent at `pid`, which must be this process's child and not yet
/// reaped; `notify` gets [`TOO_MANY_PROGRAMS`] at the first trip. Nothing when the agent has
/// exited already. An error when the kqueue or the thread cannot be made: the caller ends the
/// agent rather than run it uncapped.
pub(crate) fn watch(pid: u32, notify: Notify) -> io::Result<()> {
    let Some(start) = libproc::start_of(pid) else {
        return Ok(());
    };
    let Some(queue) = Queue::new(pid)? else {
        return Ok(());
    };
    let root = Root { pid, start };
    thread::Builder::new()
        .name("clauseo-process-cap".to_owned())
        .spawn(move || run(root, &queue, &*notify))?;
    Ok(())
}

/// The watchdog's loop, until the agent exits or is killed.
fn run(root: Root, queue: &Queue, notify: &(dyn Fn(&str) + Send + Sync)) {
    let mut told = false;
    let mut sweep_at = now() + SWEEP;
    loop {
        match queue.wait(POLL) {
            Wake::Exit | Wake::Failed => return,
            Wake::Tick | Wake::Fork => {}
        }
        let sweep = now() >= sweep_at;
        if sweep {
            sweep_at = now() + SWEEP;
        }
        // The group holds the agent itself: a count past the cap is the cap plus one.
        let crowded = libproc::group_size(root.pid).is_some_and(|n| n > AGENT_PROCESS_CAP + 1);
        if !sweep && !crowded {
            continue;
        }
        if libproc::start_of(root.pid) != Some(root.start) {
            return;
        }
        // A listing that failed is tried again at the next wake.
        let Some(found) = read(root) else {
            continue;
        };
        if found.len() <= AGENT_PROCESS_CAP {
            continue;
        }
        let Some(verdict) = trip(root) else {
            return;
        };
        if !told {
            told = true;
            notify(TOO_MANY_PROGRAMS);
        }
        if verdict == Verdict::KillAgent {
            return;
        }
    }
}

/// The agent's running descendants; `None` when the agent is gone (or its pid names another
/// process now) or the listing failed.
fn read(root: Root) -> Option<Vec<Proc>> {
    if libproc::start_of(root.pid) != Some(root.start) {
        return None;
    }
    Some(descendants(root.pid, root.start, &libproc::all()?))
}

/// Stops and kills every descendant of the agent, and the agent too when [`verdict`] says so;
/// else the agent goes on. `None` when the agent is gone.
///
/// One `killpg(SIGSTOP)` freezes the agent's whole group, the agent with it, so nothing in it
/// forks from then on. Each listed descendant is then stopped on its own (those outside the
/// group), until a listing finds none new; each one stopped is killed, and so is anything a
/// listing still finds. Only then does the agent get `SIGCONT` (or `SIGKILL`): it was stopped
/// for the trip only. Every signal but the group's goes to a pid whose start still matches.
fn trip(root: Root) -> Option<Verdict> {
    if libproc::start_of(root.pid) != Some(root.start) {
        return None;
    }
    // The agent leads its own session, so its group id is its pid and no process outside its
    // tree is in the group; the agent is unreaped, so the id is not another's.
    libproc::signal_group(root.pid, libc::SIGSTOP);
    let mut stopped: Vec<Proc> = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..STOP_ROUNDS {
        let Some(found) = read(root) else { break };
        let fresh: Vec<Proc> = found
            .into_iter()
            .filter(|p| seen.insert((p.pid, p.start)))
            .collect();
        if fresh.is_empty() {
            break;
        }
        for process in &fresh {
            libproc::signal(process.pid, process.start, libc::SIGSTOP);
        }
        stopped.extend(fresh);
    }
    let verdict = verdict(root.pid, &stopped);
    for process in &stopped {
        libproc::signal(process.pid, process.start, libc::SIGKILL);
    }
    for _ in 0..KILL_ROUNDS {
        match read(root) {
            Some(left) if !left.is_empty() => {
                for process in &left {
                    libproc::signal(process.pid, process.start, libc::SIGKILL);
                }
            }
            _ => break,
        }
        thread::park_timeout(KILL_PAUSE);
    }
    let last = match verdict {
        Verdict::KillAgent => libc::SIGKILL,
        Verdict::SpareAgent => libc::SIGCONT,
    };
    libproc::signal(root.pid, root.start, last);
    tracing::warn!(
        count = stopped.len(),
        ?verdict,
        "the agent's process cap stopped its descendants"
    );
    Some(verdict)
}

/// The monotonic clock the sweep's schedule runs on.
#[expect(
    clippy::disallowed_methods,
    reason = "the watchdog's schedule: a real-time loop in its own thread, not core logic"
)]
fn now() -> Instant {
    Instant::now()
}
