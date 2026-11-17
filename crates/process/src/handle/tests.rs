use std::pin::pin;

use async_io::block_on;
use futures::channel::oneshot;
use futures::join;

use super::*;
use crate::timer::ManualTimer;

/// The escalation steps the ladder takes before its last wait, on this OS.
fn every_step() -> Vec<Step> {
    #[cfg(unix)]
    return vec![Step::Terminate, Step::Kill];
    #[cfg(windows)]
    return vec![Step::Kill];
}

/// Ends the 2 s wait and, on Unix, the 3 s wait, as a test that waits out each step would.
async fn fire_until_the_kill(timer: &ManualTimer) {
    assert_eq!(timer.next_armed().await, STDIN_GRACE);
    assert!(timer.fire());
    #[cfg(unix)]
    {
        assert_eq!(timer.next_armed().await, TERM_GRACE);
        assert!(timer.fire());
    }
}

#[test]
fn an_agent_that_never_exits_is_unresponsive() {
    let timer = ManualTimer::new();
    let mut steps = Vec::new();
    let exit = pin!(futures::future::pending::<()>());
    let script = async {
        fire_until_the_kill(&timer).await;
        assert_eq!(timer.next_armed().await, KILL_GRACE);
        assert!(timer.fire());
    };
    let (ended, ()) = block_on(async {
        join!(
            ladder(exit, async {}, &timer, STDIN_GRACE, |step| steps.push(step)),
            script
        )
    });
    assert!(matches!(ended, Ended::Unresponsive), "{ended:?}");
    assert_eq!(steps, every_step());
    assert_eq!(timer.armed(), timer.fired());
}

#[test]
fn an_exit_after_the_kill_is_killed() {
    let timer = ManualTimer::new();
    let mut steps = Vec::new();
    let (exited, exit) = oneshot::channel::<i32>();
    let mut exited = Some(exited);
    let exit = pin!(exit);
    let act = |step| {
        steps.push(step);
        if step == Step::Kill {
            exited.take().map(|exited| exited.send(9));
        }
    };
    let script = async {
        fire_until_the_kill(&timer).await;
        // The last wait is armed, and the exit comes before it fires.
        assert_eq!(timer.next_armed().await, KILL_GRACE);
    };
    let (ended, ()) =
        block_on(async { join!(ladder(exit, async {}, &timer, STDIN_GRACE, act), script) });
    assert!(matches!(ended, Ended::Killed(Ok(9))), "{ended:?}");
    assert_eq!(steps, every_step());
    assert_eq!(timer.fired() + 1, timer.armed());
}

/// Stdin's close ends before the exit is waited for: an agent that would end at once still
/// gets what was written first (on Windows, a write still in `blocking::Unblock`'s buffer).
#[test]
fn the_exit_is_waited_for_once_stdin_is_closed() {
    let timer = ManualTimer::new();
    let closed = std::cell::Cell::new(false);
    let close = async {
        futures::future::ready(()).await;
        closed.set(true);
    };
    let exit = pin!(async {
        assert!(closed.get(), "the exit was waited for before stdin closed");
        0
    });
    let ended = block_on(ladder(exit, close, &timer, STDIN_GRACE, |_| {
        panic!("no step")
    }));
    assert!(matches!(ended, Ended::StdinClosed(0)), "{ended:?}");
}

/// A close that never ends (a pipe the agent stopped reading) is dropped at the stdin grace,
/// and the ladder goes on.
#[test]
fn a_close_that_never_ends_costs_only_the_stdin_grace() {
    let timer = ManualTimer::new();
    let mut steps = Vec::new();
    let exit = pin!(futures::future::pending::<()>());
    let script = async {
        fire_until_the_kill(&timer).await;
        assert_eq!(timer.next_armed().await, KILL_GRACE);
        assert!(timer.fire());
    };
    let close = futures::future::pending::<()>();
    let (ended, ()) = block_on(async {
        join!(
            ladder(exit, close, &timer, STDIN_GRACE, |step| steps.push(step)),
            script
        )
    });
    assert!(matches!(ended, Ended::Unresponsive), "{ended:?}");
    assert_eq!(steps, every_step());
}

#[cfg(windows)]
mod windows;
