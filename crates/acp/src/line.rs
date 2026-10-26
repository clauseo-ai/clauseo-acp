//! The stdout line path and the stdin line sink.
//!
//! The SDK's `Lines` transport takes its lines from [`incoming`], which splits the agent's stdout
//! into lines and passes on only the ones that are UTF-8 JSON. Without it, SDK 3.3.0's `Lines`
//! answers a malformed line with a parse-error response to the agent, which a strict agent (and
//! `fake_agent`) treats as a protocol error; here the line is skipped with
//! `ProtocolWarning::MalformedLine` and nothing goes back. `Lines` caps nothing, so the 8 MiB
//! cap is here too.
//!
//! Hot path: every agent message takes it. The measured choices (64 KiB buffer, `memchr`,
//! `from_utf8` then `IgnoredAny`, one exact allocation for a line that sits whole in the buffer)
//! are in the pull request that added them.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use futures::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use futures::task::AtomicWaker;
use futures::{Sink, Stream};

use crate::event::FrameDirection;
use crate::state::Shared;

#[cfg(test)]
mod tests;

/// The longest JSON-RPC line the agent may send: 8 MiB, newline excluded.
pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// The stdout read buffer. A macOS pipe holds at most 64 KiB, so a larger one gains nothing.
const READ_BUFFER_BYTES: usize = 64 * 1024;

/// Stops the line reader while an event waits for room in the full event channel, so a host
/// that stops draining slows the agent instead of filling memory. The SDK reads lines into an
/// unbounded queue, so the reader itself has to stop.
#[derive(Debug, Default)]
pub(crate) struct Gate {
    /// How many events are waiting for room.
    blocked: AtomicUsize,
    /// The line reader, when it waits for the gate.
    reader: AtomicWaker,
}

impl Gate {
    /// An event starts waiting for room.
    pub(crate) fn close(&self) {
        self.blocked.fetch_add(1, Ordering::AcqRel);
    }

    /// An event found room.
    pub(crate) fn open(&self) {
        if self.blocked.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.reader.wake();
        }
    }

    /// Ready when no event waits for room.
    fn poll_open(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.blocked.load(Ordering::Acquire) == 0 {
            return Poll::Ready(());
        }
        self.reader.register(cx.waker());
        if self.blocked.load(Ordering::Acquire) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Why [`LineSplitter::next_line`] stopped.
#[derive(Debug)]
pub(crate) enum SplitError {
    /// A line grew past [`MAX_LINE_BYTES`].
    TooLong,
    /// Reading the pipe failed.
    Io(io::Error),
}

/// Splits a byte stream into lines with `memchr` over the read buffer.
pub(crate) struct LineSplitter<R> {
    reader: BufReader<R>,
    /// The start of a line that did not end in the bytes read so far.
    partial: Vec<u8>,
}

impl<R: AsyncRead + Unpin> LineSplitter<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::with_capacity(READ_BUFFER_BYTES, reader),
            partial: Vec::new(),
        }
    }

    /// The next line without its `\n`, or `None` at the end of the stream. A last line with no
    /// `\n` still counts.
    pub(crate) async fn next_line(&mut self) -> Result<Option<Vec<u8>>, SplitError> {
        loop {
            let buffer = match self.reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(SplitError::Io(error)),
            };
            if buffer.is_empty() {
                return Ok((!self.partial.is_empty()).then(|| std::mem::take(&mut self.partial)));
            }
            if let Some(end) = memchr::memchr(b'\n', buffer) {
                let (head, _) = buffer.split_at(end);
                let line = if self.partial.is_empty() {
                    // The common case: the whole line is in the buffer. One exact allocation.
                    if head.len() > MAX_LINE_BYTES {
                        return Err(SplitError::TooLong);
                    }
                    head.to_vec()
                } else {
                    self.partial.extend_from_slice(head);
                    if self.partial.len() > MAX_LINE_BYTES {
                        return Err(SplitError::TooLong);
                    }
                    std::mem::take(&mut self.partial)
                };
                self.reader.consume_unpin(end + 1);
                return Ok(Some(line));
            }
            let read = buffer.len();
            self.partial.extend_from_slice(buffer);
            self.reader.consume_unpin(read);
            if self.partial.len() > MAX_LINE_BYTES {
                return Err(SplitError::TooLong);
            }
        }
    }
}

/// The lines the SDK's `Lines` transport reads: each one UTF-8 JSON, copied to the frame tap.
/// Blank lines are skipped; any other line that is not UTF-8 JSON becomes a
/// [`MALFORMED_LINE_METHOD`] notification, which the inbound handler turns into a
/// [`ProtocolWarning::MalformedLine`](crate::ProtocolWarning::MalformedLine). A line over 8 MiB ends the stream with an error, which
/// closes the connection (the driver reports `Error::Protocol`).
pub(crate) fn incoming<R>(
    reader: R,
    shared: Arc<Shared>,
) -> impl Stream<Item = io::Result<String>> + Send + 'static
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let splitter = LineSplitter::new(reader);
    futures::stream::unfold(Some((splitter, shared)), |state| async move {
        let (mut splitter, shared) = state?;
        loop {
            futures::future::poll_fn(|cx| shared.gate.poll_open(cx)).await;
            let bytes = match splitter.next_line().await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return None,
                Err(SplitError::TooLong) => {
                    shared.set_fatal(crate::Error::Protocol(format!(
                        "the agent sent a line over {MAX_LINE_BYTES} bytes"
                    )));
                    let error = io::Error::new(io::ErrorKind::InvalidData, "line over 8 MiB");
                    return Some((Err(error), None));
                }
                Err(SplitError::Io(error)) => return Some((Err(error), None)),
            };
            if bytes
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
            {
                continue;
            }
            let line = match String::from_utf8(bytes) {
                Ok(line) if serde_json::from_str::<serde::de::IgnoredAny>(&line).is_ok() => {
                    shared.tap(FrameDirection::In, &line);
                    line
                }
                Ok(line) => {
                    shared.tap(FrameDirection::In, &line);
                    malformed_marker(line.len())
                }
                Err(error) => {
                    shared.tap(
                        FrameDirection::In,
                        &String::from_utf8_lossy(error.as_bytes()),
                    );
                    malformed_marker(error.as_bytes().len())
                }
            };
            return Some((Ok(line), Some((splitter, shared))));
        }
    })
}

/// The method of the notification that stands in for a malformed line, so its warning reaches
/// the host in order with the agent's other messages: the SDK queues lines ahead of its dispatch
/// loop, and a warning sent from here would overtake them.
pub(crate) const MALFORMED_LINE_METHOD: &str = "_clauseo/malformed_line";

fn malformed_marker(bytes: usize) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"{MALFORMED_LINE_METHOD}","params":{{"bytes":{bytes}}}}}"#
    )
}

/// Writes each line and a `\n` to the agent's stdin, flushing after each, and copies it to the
/// frame tap. Closing it closes the writer, which the agent reads as end of input.
pub(crate) struct LineSink<W> {
    writer: W,
    shared: Arc<Shared>,
    bytes: Vec<u8>,
    written: usize,
}

impl<W> LineSink<W> {
    pub(crate) fn new(writer: W, shared: Arc<Shared>) -> Self {
        Self {
            writer,
            shared,
            bytes: Vec::new(),
            written: 0,
        }
    }
}

impl<W: AsyncWrite + Unpin> Sink<String> for LineSink<W> {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: String) -> io::Result<()> {
        let this = self.get_mut();
        this.shared.tap(FrameDirection::Out, &item);
        this.bytes = item.into_bytes();
        this.bytes.push(b'\n');
        this.written = 0;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        while let Some(rest) = this
            .bytes
            .get(this.written..)
            .filter(|rest| !rest.is_empty())
        {
            let count = futures::ready!(Pin::new(&mut this.writer).poll_write(cx, rest))?;
            if count == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            this.written += count;
        }
        this.bytes.clear();
        this.written = 0;
        Pin::new(&mut this.writer).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        futures::ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.get_mut().writer).poll_close(cx)
    }
}
