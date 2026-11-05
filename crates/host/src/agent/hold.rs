//! A test seam on an agent's connection: once a given number of lines has been written to its
//! stdin, the wakes of the task that runs its [`Driver`] are held back until the test releases
//! them, as a host thread that has not got round to that task yet holds them. Every wake is
//! held (the stdout reader's, and the one `ReadProgress::mark` sends), and nothing else changes:
//! a poll still reads what the pipe holds.

use std::cell::RefCell;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use clauseo_acp::{AcpClient, AcpEvent, ConnectOptions, Driver};
use futures::io::{AsyncRead, AsyncWrite};

thread_local! {
    /// The hold the next agent started on this thread takes.
    static NEXT: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

/// The test's handle on a hold.
pub(crate) struct ConnectionHold(Arc<Shared>);

impl ConnectionHold {
    /// Holds the connection task's wakes of the next agent this thread starts once `lines`
    /// lines have been written to its stdin.
    pub(crate) fn after_written(lines: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                lines_left: lines,
                task: None,
                woken: false,
                watcher: None,
            }),
        });
        NEXT.with(|next| *next.borrow_mut() = Some(Arc::clone(&shared)));
        Self(shared)
    }

    /// Ends once the connection task was woken and the wake is held.
    pub(crate) fn wake_held(&self) -> impl Future<Output = ()> + '_ {
        futures::future::poll_fn(|cx| {
            let mut state = self.0.lock();
            if state.woken {
                return Poll::Ready(());
            }
            state.watcher = Some(cx.waker().clone());
            Poll::Pending
        })
    }

    /// Lets the wakes through, the held one first.
    pub(crate) fn release(&self) {
        let task = {
            let mut state = self.0.lock();
            state.lines_left = usize::MAX;
            let woken = std::mem::take(&mut state.woken);
            state.task.take().filter(|_| woken)
        };
        if let Some(task) = task {
            task.wake();
        }
    }
}

struct Shared {
    state: Mutex<State>,
}

struct State {
    /// Lines still to write before wakes are held; 0 holds them, `usize::MAX` never does.
    lines_left: usize,
    /// The connection task's waker, from its last poll.
    task: Option<Waker>,
    /// A wake came while held.
    woken: bool,
    /// The test, waiting for a held wake.
    watcher: Option<Waker>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Wake for Shared {
    fn wake(self: Arc<Self>) {
        let (task, watcher) = {
            let mut state = self.lock();
            if state.lines_left == 0 {
                state.woken = true;
                (None, state.watcher.take())
            } else {
                (state.task.clone(), None)
            }
        };
        for waker in [task, watcher].into_iter().flatten() {
            waker.wake();
        }
    }
}

/// `acp`'s `connect`, with the hold set on this thread (taken: one agent per hold), if any.
pub(crate) fn connect<W, R>(
    stdin: W,
    stdout: R,
) -> (AcpClient, async_channel::Receiver<AcpEvent>, HeldTask)
where
    W: AsyncWrite + Send + Unpin + 'static,
    R: AsyncRead + Send + Unpin + 'static,
{
    let hold = NEXT.with(|next| next.borrow_mut().take());
    let stdin = Counted {
        writer: stdin,
        hold: hold.clone(),
    };
    let (client, events, driver) = clauseo_acp::connect(stdin, stdout, ConnectOptions::default());
    (client, events, HeldTask { driver, hold })
}

/// The agent's stdin, counting the lines written toward the hold.
struct Counted<W> {
    writer: W,
    hold: Option<Arc<Shared>>,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Counted<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = Pin::new(&mut this.writer).poll_write(cx, buf);
        if let (Some(hold), Poll::Ready(Ok(n))) = (&this.hold, &written) {
            let lines = buf.iter().take(*n).filter(|byte| **byte == b'\n').count();
            let mut state = hold.lock();
            if state.lines_left != usize::MAX {
                state.lines_left = state.lines_left.saturating_sub(lines);
            }
        }
        written
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_close(cx)
    }
}

/// The [`Driver`], polled with a waker the hold can hold back.
pub(crate) struct HeldTask {
    driver: Driver,
    hold: Option<Arc<Shared>>,
}

impl Future for HeldTask {
    type Output = <Driver as Future>::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(hold) = &this.hold else {
            return Pin::new(&mut this.driver).poll(cx);
        };
        hold.lock().task = Some(cx.waker().clone());
        let proxy = Waker::from(Arc::clone(hold));
        Pin::new(&mut this.driver).poll(&mut Context::from_waker(&proxy))
    }
}
