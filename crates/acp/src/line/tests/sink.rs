//! The [`LineSink`] keeps the `Sink` contract: every line fed reaches the writer in order, and
//! closing the sink closes the writer.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::task::{Context, Poll};

use futures::{AsyncWrite, SinkExt};

use crate::line::LineSink;
use crate::state::Shared;

/// The agent's stdin: what was written, and whether it was closed.
#[derive(Default)]
struct Stdin {
    written: Vec<u8>,
    closed: bool,
}

impl AsyncWrite for Stdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        assert!(!this.closed, "write after close");
        this.written.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().closed = true;
        Poll::Ready(Ok(()))
    }
}

fn sink() -> LineSink<Stdin> {
    let (events, _) = async_channel::bounded(1);
    let (stop, _) = async_channel::bounded(1);
    let shared = Shared::new(events, None, Arc::new(AtomicU64::new(0)), stop);
    LineSink::new(Stdin::default(), Arc::new(shared))
}

/// Lines fed one after another without a flush in between (as `send_all` does) all reach the
/// writer, each with its newline, in order.
#[test]
fn lines_fed_without_a_flush_all_reach_the_writer() {
    let mut sink = sink();
    futures::executor::block_on(async {
        sink.feed("first".to_owned()).await.unwrap();
        sink.feed("second".to_owned()).await.unwrap();
        sink.flush().await.unwrap();
    });
    assert_eq!(sink.writer.written, b"first\nsecond\n");
}

/// Closing the sink writes the line still pending, then closes the writer: the agent reads the
/// end of its input.
#[test]
fn closing_the_sink_writes_what_is_pending_and_closes_the_writer() {
    let mut sink = sink();
    futures::executor::block_on(async {
        sink.feed("last".to_owned()).await.unwrap();
        sink.close().await.unwrap();
    });
    assert_eq!(sink.writer.written, b"last\n");
    assert!(sink.writer.closed, "the writer was not closed");
}
