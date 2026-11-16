//! A call the command loop never read is answered `Closed` when the driver ends (private#513):
//! the SDK can end the connection on an actor's error (a write to a closed stdin) in a poll
//! that does not poll the command loop again, so a call queued just before was left with its
//! reply alive in the channel, and its caller waited for good.

use std::sync::{Arc, Mutex};
use std::task::Waker;

use futures::FutureExt as _;

use super::*;

/// The agent's stdin, held: each write waits until the test fails it, as a stdin the agent
/// stopped reading and then closed does.
#[derive(Clone, Default)]
struct HeldStdin(Arc<Mutex<Held>>);

#[derive(Default)]
struct Held {
    broken: bool,
    waker: Option<Waker>,
}

impl HeldStdin {
    /// Fails the write waiting, and every later one, with `BrokenPipe`.
    fn break_pipe(&self) {
        let waker = {
            let mut held = self.0.lock().unwrap();
            held.broken = true;
            held.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn poll(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut held = self.0.lock().unwrap();
        if held.broken {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        held.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl AsyncWrite for HeldStdin {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        self.poll(cx).map_ok(|()| 0)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll(cx)
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn turn() -> TurnId {
    TurnId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap()
}

/// A connection whose `initialize` is read and whose write then fails, polled `polls` times
/// after the failure before `session/new` is queued; then polled to its end. Returns whether
/// `session/new` was answered.
fn queued_after_polls(polls: usize) -> bool {
    let stdin = HeldStdin::default();
    let (_stdout, stdout_rx) = mpsc::unbounded();
    let stdout_rx = Stdout {
        chunks: stdout_rx,
        rest: Vec::new(),
    };
    let (client, _events, mut driver) =
        connect(stdin.clone(), stdout_rx, ConnectOptions::default());
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut initialize = client.initialize().boxed();
    assert!(initialize.poll_unpin(&mut cx).is_pending());
    // The loop reads `initialize`; its line's write waits, then fails.
    assert!(driver.poll_unpin(&mut cx).is_pending());
    stdin.break_pipe();
    let mut ended = false;
    for _ in 0..polls {
        if driver.poll_unpin(&mut cx).is_ready() {
            ended = true;
            break;
        }
    }
    let new = NewSession {
        cwd: cwd(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        meta: None,
    };
    let mut new_session = client.new_session(new).boxed();
    if let Poll::Ready(answer) = new_session.poll_unpin(&mut cx) {
        // Queued after the driver ended: refused at once.
        return matches!(answer, Err(Error::Closed));
    }
    for _ in 0..100 {
        if ended || driver.poll_unpin(&mut cx).is_ready() {
            ended = true;
            break;
        }
    }
    assert!(ended, "the driver did not end after its stdin failed");
    matches!(
        new_session.poll_unpin(&mut cx),
        Poll::Ready(Err(Error::Closed))
    )
}

#[test]
fn a_call_queued_as_the_connection_fails_is_answered_closed() {
    // Polled once after the failure, the SDK ends the connection without reading the call
    // queued meanwhile; the other counts cover the polls before and after that one.
    for polls in 0..20 {
        assert!(
            queued_after_polls(polls),
            "polls={polls}: a call the loop never read is still waiting"
        );
    }
}

#[test]
fn a_call_queued_before_the_driver_is_dropped_is_answered_closed() {
    let (client, _events, driver, _agent) = pipes();
    let session = AgentSessionId::new("s-1").unwrap();
    let mut prompt = Box::pin(client.prompt(&session, turn(), Vec::new()));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(prompt.poll_unpin(&mut cx).is_pending());
    // Never polled, so its loop never read the prompt.
    drop(driver);
    assert!(matches!(
        prompt.poll_unpin(&mut cx),
        Poll::Ready(Err(Error::Closed))
    ));
}
