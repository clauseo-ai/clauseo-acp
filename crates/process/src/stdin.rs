//! The agent's stdin, which its writer and its handle share, so shutdown can close it without the
//! writer's help.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use async_process::ChildStdin;
use futures::AsyncWrite;

/// The pipe, until shutdown takes it, and the waker of a write that waits for room in it.
#[derive(Debug)]
struct Slot {
    pipe: Option<ChildStdin>,
    waiting: Option<Waker>,
}

/// The slot [`AgentStdin`] and its [`Handle`](crate::Handle) share.
#[derive(Debug, Clone)]
pub(crate) struct StdinSlot(Arc<Mutex<Slot>>);

impl StdinSlot {
    pub(crate) fn new(pipe: ChildStdin) -> StdinSlot {
        StdinSlot(Arc::new(Mutex::new(Slot {
            pipe: Some(pipe),
            waiting: None,
        })))
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Closes the pipe: the agent reads end of file, and later writes fail with `BrokenPipe`. A
    /// write that waits for room wakes and fails the same way.
    pub(crate) fn close(&self) {
        let (pipe, waiting) = {
            let mut slot = self.lock();
            (slot.pipe.take(), slot.waiting.take())
        };
        drop(pipe);
        if let Some(waiting) = waiting {
            waiting.wake();
        }
    }
}

/// The agent's stdin as a [`futures::io::AsyncWrite`]. Once the handle's shutdown closes it, or
/// [`AsyncWrite::poll_close`] does, every write fails with [`io::ErrorKind::BrokenPipe`].
#[derive(Debug)]
pub struct AgentStdin {
    slot: StdinSlot,
}

impl AgentStdin {
    pub(crate) fn new(slot: StdinSlot) -> AgentStdin {
        AgentStdin { slot }
    }

    /// Runs `op` on the pipe while it is open, and keeps the waker when the pipe has no room.
    /// `if_closed` is the answer once it is closed.
    fn with_pipe<T>(
        &self,
        cx: &mut Context<'_>,
        if_closed: impl FnOnce() -> io::Result<T>,
        op: impl FnOnce(Pin<&mut ChildStdin>, &mut Context<'_>) -> Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        let mut slot = self.slot.lock();
        let Some(pipe) = slot.pipe.as_mut() else {
            return Poll::Ready(if_closed());
        };
        let poll = op(Pin::new(pipe), cx);
        if poll.is_pending() {
            slot.waiting = Some(cx.waker().clone());
        }
        poll
    }
}

impl AsyncWrite for AgentStdin {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.with_pipe(cx, closed, |pipe, cx| pipe.poll_write(cx, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.with_pipe(cx, closed, AsyncWrite::poll_flush)
    }

    /// Flushes and closes the pipe. Closing it again is fine.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let poll = self.with_pipe(cx, || Ok(()), AsyncWrite::poll_close);
        if poll.is_ready() {
            self.slot.close();
        }
        poll
    }
}

/// What a write or flush gets once the pipe is closed.
fn closed<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "the agent's stdin is closed",
    ))
}
