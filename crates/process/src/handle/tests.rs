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
            ladder(exit, &timer, Grace::Ladder, |step| steps.push(step)),
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
    let (ended, ()) = block_on(async { join!(ladder(exit, &timer, Grace::Ladder, act), script) });
    assert!(matches!(ended, Ended::Killed(Ok(9))), "{ended:?}");
    assert_eq!(steps, every_step());
    assert_eq!(timer.fired() + 1, timer.armed());
}

/// With no grace the kill comes first, before any wait: the only timer is the last 2 s.
#[test]
fn a_stop_with_no_grace_kills_at_once() {
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
        assert_eq!(timer.next_armed().await, KILL_GRACE);
    };
    let (ended, ()) = block_on(async { join!(ladder(exit, &timer, Grace::None, act), script) });
    assert!(matches!(ended, Ended::Killed(Ok(9))), "{ended:?}");
    assert_eq!(steps, [Step::Kill]);
    assert_eq!(timer.fired(), 0);
}

#[cfg(windows)]
mod windows;
