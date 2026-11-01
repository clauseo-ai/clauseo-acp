//! A copy of what a reader's program writes to stdout, for G16's live recordings
//! (`tests/live.rs`). Nothing in the product sets one: without a tap the stdout passes as it is.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use clauseo_protocol::AgentKind;
use futures::AsyncRead;

use super::run::Stdout;

/// Gets each chunk a reader's program writes to stdout, as the reader reads it, with the agent.
/// The bytes are raw and unredacted: whoever sets a tap redacts before keeping anything.
#[cfg(feature = "live-tap")]
#[doc(hidden)]
pub type StdoutTap = Arc<dyn Fn(AgentKind, &[u8]) + Send + Sync>;
/// [`StdoutTap`] without the `live-tap` feature: crate-private, and never set.
#[cfg(not(feature = "live-tap"))]
pub(crate) type StdoutTap = Arc<dyn Fn(AgentKind, &[u8]) + Send + Sync>;

/// `stdout`, copied to `tap` as it is read when there is one.
pub(crate) fn tapped(stdout: Stdout, agent: AgentKind, tap: Option<StdoutTap>) -> Stdout {
    match tap {
        None => stdout,
        Some(tap) => Box::new(Tapped {
            inner: stdout,
            agent,
            tap,
        }),
    }
}

/// A stdout whose reads are copied to a tap.
struct Tapped {
    /// The program's stdout.
    inner: Stdout,
    /// The agent the reader reads.
    agent: AgentKind,
    /// Where the copies go.
    tap: StdoutTap,
}

impl AsyncRead for Tapped {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let polled = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(read)) = &polled
            && let Some(chunk) = buf.get(..*read)
            && !chunk.is_empty()
        {
            (this.tap)(this.agent, chunk);
        }
        polled
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures::AsyncReadExt as _;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn a_tap_gets_every_byte_read_and_the_reader_gets_them_too() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        let tap: StdoutTap = Arc::new(move |agent, chunk| {
            assert_eq!(agent, AgentKind::Codex);
            copy.lock().unwrap().extend_from_slice(chunk);
        });
        let stdout: Stdout = Box::new(futures::io::Cursor::new(b"one\ntwo\n".to_vec()));
        let mut stdout = tapped(stdout, AgentKind::Codex, Some(tap));
        let mut read = Vec::new();
        block_on(stdout.read_to_end(&mut read)).unwrap();
        assert_eq!(read, b"one\ntwo\n");
        assert_eq!(*seen.lock().unwrap(), b"one\ntwo\n");
    }
}
