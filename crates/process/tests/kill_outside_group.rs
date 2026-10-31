//! The shutdown ladder's kill reaches an agent that left the process group it was started in
//! (Unix). The agent is this test binary run again (`support/self_agent.rs`), since no
//! `fake_agent` scenario changes its group.

#![cfg(unix)]

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::io::Write as _;
use std::pin::pin;
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{Launched, ManualTimer, ShutdownOutcome, SpawnOptions};
use futures::channel::mpsc;
use futures::future::{Either, select};
use futures::{AsyncReadExt, StreamExt, join};
use nix::sys::signal::{Signal, kill};
use nix::unistd::{Pid, getpgid, getppid, setpgid};

/// The line the agent writes once it is out of its group.
const LEFT: &str = "agent: left its process group";

/// The agent the test here starts, not a check: run as one, it moves into the process group of
/// the test that started it, says so on stderr and holds until killed. In a normal run it
/// returns at once.
#[test]
fn agent_leaves_its_group_and_holds() {
    if !self_agent::is_agent() {
        return;
    }
    let group = getpgid(Some(getppid())).expect("the test's process group");
    setpgid(Pid::from_raw(0), group).expect("join the test's process group");
    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "{LEFT}").expect("write stderr");
    stderr.flush().expect("flush stderr");
    drop(stderr);
    hold();
}

/// SIGKILLs the agent's pid when dropped, as when the test panics, unless the pid was taken out.
struct KillOnDrop(Option<Pid>);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            kill(pid, Signal::SIGKILL).unwrap_or_default();
        }
    }
}

fn hold() -> ! {
    loop {
        std::thread::park();
    }
}

/// An agent that moved out of its process group, so the group's SIGTERM and SIGKILL reach
/// nobody, is still killed by the ladder's last step (the kill of the agent's own pid), and
/// the outcome says `Killed` by SIGKILL.
#[test]
fn ladder_kills_an_agent_that_left_its_group() {
    let Launched {
        mut stdout,
        stderr,
        handle,
        ..
    } = self_agent::launch("agent_leaves_its_group_and_holds", &SpawnOptions::new()).unwrap();
    // Out of its group, the agent outlives the handle's tree kill; the guard ends it if the test
    // fails before the ladder does.
    let mut guard = KillOnDrop(Some(Pid::from_raw(i32::try_from(handle.pid()).unwrap())));
    let (tx, mut lines) = mpsc::unbounded();
    let pumped = stderr.lines(move |line| drop(tx.unbounded_send(line.to_owned())));
    let timer = ManualTimer::new();
    let script = async {
        while let Some(line) = lines.next().await {
            if line == LEFT {
                break;
            }
        }
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        assert!(timer.fire());
        assert_eq!(timer.next_armed().await, Duration::from_secs(3));
        assert!(timer.fire());
        // The wait for the exit after the kill. The agent's stdout closes when it dies; if it
        // has not within 10 s, the kill missed it, and firing ends the ladder as Unresponsive.
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        let mut out = Vec::new();
        let closed = pin!(stdout.read_to_end(&mut out));
        if let Either::Right(_) = select(closed, Timer::after(Duration::from_secs(10))).await {
            timer.fire();
        }
    };
    let outcome = block_on(async {
        // The pump ends only once the agent is dead, so it is not waited for.
        let ladder = pin!(async { join!(handle.shutdown(&timer), script).0 });
        match select(ladder, pin!(pumped)).await {
            Either::Left((outcome, _)) => outcome,
            Either::Right((_, ladder)) => ladder.await,
        }
    });
    let ShutdownOutcome::Killed(report) = outcome else {
        panic!("expected Killed, got {outcome:?}");
    };
    // It is dead and reaped, so its pid may be reused: the guard must not kill it.
    guard.0 = None;
    assert_eq!(report.signal(), Some(Signal::SIGKILL as i32));
}
