//! A test seam on the agent's stdout: once a given number of lines has passed, the reader's
//! wakes are held back until the test releases them, as a host thread that has not got round to
//! the reader yet holds them. Reads still pass through, so whatever polls the reader reads what
//! the pipe holds; only the wake that would poll it is late.

use std::cell::RefCell;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use futures::io::AsyncRead;

thread_local! {
    /// The hold the next agent started on this thread takes.
    static NEXT: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) };
}

/// The test's handle on a hold.
pub(crate) struct StdoutHold(Arc<Shared>);

impl StdoutHold {
    /// Holds the wakes of the next agent this thread starts once `lines` lines of its stdout
    /// have been read.
    pub(crate) fn after_lines(lines: usize) -> Self {
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

    /// Ends once the reader was woken and the wake is held.
    pub(crate) fn wake_held(&self) -> impl std::future::Future<Output = ()> + '_ {
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
            state.woken = false;
            state.task.take()
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
    /// Lines still to pass before wakes are held; 0 holds them, `usize::MAX` never does.
    lines_left: usize,
    /// The reader task's waker, from its last poll.
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
        let task = {
            let mut state = self.lock();
            if state.lines_left == 0 {
                state.woken = true;
                state.watcher.take()
            } else {
                state.task.take()
            }
        };
        if let Some(task) = task {
            task.wake();
        }
    }
}

/// The stdout an agent started on this thread reads through: held when a test set a hold.
pub(crate) struct Held<R> {
    reader: R,
    hold: Option<Arc<Shared>>,
}

/// `stdout`, with the hold set on this thread, if any (taken: one agent per hold).
pub(crate) fn held<R>(stdout: R) -> Held<R> {
    Held {
        reader: stdout,
        hold: NEXT.with(|next| next.borrow_mut().take()),
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Held<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let Some(hold) = &this.hold else {
            return Pin::new(&mut this.reader).poll_read(cx, buf);
        };
        hold.lock().task = Some(cx.waker().clone());
        let proxy = Waker::from(Arc::clone(hold));
        let read = Pin::new(&mut this.reader).poll_read(&mut Context::from_waker(&proxy), buf);
        if let Poll::Ready(Ok(n)) = &read {
            let lines = buf.iter().take(*n).filter(|byte| **byte == b'\n').count();
            let mut state = hold.lock();
            if state.lines_left != usize::MAX {
                state.lines_left = state.lines_left.saturating_sub(lines);
            }
        }
        read
    }
}
