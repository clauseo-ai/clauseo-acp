//! The stdout line path and the stdin line sink.
//!
//! The SDK's `Lines` transport takes its lines from [`incoming`], which splits the agent's stdout
//! into lines and passes on only the ones that are UTF-8 JSON-RPC messages. Without it, SDK
//! 3.3.0's `Lines` answers a line that is not one (not JSON, or JSON such as a log object, a
//! number or an array) with an error response to the agent, which a strict agent (and
//! `fake_agent`) treats as a protocol error; here the line is skipped with
//! `ProtocolWarning::MalformedLine` and nothing goes back. `Lines` caps nothing, so the 8 MiB
//! cap and the [`Gate`] are here too.
//!
//! The SDK decodes each line it gets as a `serde_json` `Value` and then as a
//! `RawJsonRpcMessage`, and answers a line that fails either with an error response too, so
//! [`Envelope`]'s decode refuses every line either refuses or reads differently (see
//! `envelope.rs`).
//!
//! Hot path: every agent message takes it. The measured choices (64 KiB buffer, `memchr`,
//! `from_utf8` then a decode that builds no `Value`, escaped strings going through
//! `serde_json`'s scratch buffer, one exact allocation for a line that sits whole in the buffer)
//! are in pull request #64; checking values in that decode rather than building a `Value` is in
//! pull request #102.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};

use futures::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use futures::task::AtomicWaker;
use futures::{Sink, Stream};

use crate::event::FrameDirection;
use crate::state::Shared;

mod envelope;
#[cfg(test)]
mod tests;

use envelope::Envelope;

/// The longest JSON-RPC line the agent may send: 8 MiB, newline excluded.
pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// The stdout read buffer. A macOS pipe holds at most 64 KiB, so a larger one gains nothing.
const READ_BUFFER_BYTES: usize = 64 * 1024;

/// How many counted lines may wait between the line reader and the inbound handler.
pub(crate) const MAX_LINES_IN_FLIGHT: usize = 64;

/// Stops the line reader, so a host that stops draining slows the agent instead of filling
/// memory. The SDK reads lines into an unbounded queue ahead of its dispatch loop, so the reader
/// itself has to stop. It stops while an event waits for room in the full event channel, and
/// while [`MAX_LINES_IN_FLIGHT`] counted lines are read but not yet handled.
///
/// Counted lines are the `session/update` notifications and the malformed lines, the ones an
/// agent sends many of: [`incoming`] counts each when it passes it on, and the inbound handler
/// counts it off ([`counted`]) when the SDK hands it over. The agent's responses, requests and
/// other notifications are not counted, because the SDK does not hand every one of them to the
/// handler (a response to an unknown id, a malformed `$/cancel_request`), and a line counted
/// but never handled would stop the reader for good.
///
/// That the SDK hands every counted line to the handler exactly once was read from SDK 3.3.0
/// with `unstable_protocol_v2` off, as this crate builds it. With that feature on, the SDK drops
/// a notification that comes before `initialize` (`jsonrpc/protocol_compat.rs`), which would
/// leave it counted; recheck this before turning the feature on or bumping the SDK.
///
/// It also keeps what [`ReadProgress`] reads: how many times a read of the agent's stdout found
/// it empty, whether the last read did (or the stdout ended), and the waker of a read that waits,
/// which [`ReadProgress::mark`] wakes.
#[derive(Debug, Default)]
pub(crate) struct Gate {
    /// How many events are waiting for room.
    blocked: AtomicUsize,
    /// Counted lines passed on to the SDK that the inbound handler has not started.
    in_flight: AtomicUsize,
    /// The line reader, when it waits for the gate.
    reader: AtomicWaker,
    /// How many reads of the agent's stdout found it empty, plus one for its end.
    waits: AtomicU64,
    /// The last read of the agent's stdout found it empty, or the stdout ended.
    idle: AtomicBool,
    /// The read of the agent's stdout that waits, when one does.
    stdout: AtomicWaker,
}

impl Gate {
    /// The reader passed on a counted line.
    fn line_read(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    /// The inbound handler started a counted line.
    pub(crate) fn line_handled(&self) {
        let before = self
            .in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
        if before.is_ok_and(|before| before >= MAX_LINES_IN_FLIGHT) {
            self.reader.wake();
        }
    }

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

    fn is_open(&self) -> bool {
        self.blocked.load(Ordering::Acquire) == 0
            && self.in_flight.load(Ordering::Acquire) < MAX_LINES_IN_FLIGHT
    }

    /// A read of the agent's stdout found it empty and waits, to be woken by `waker` too.
    fn stdout_waits(&self, waker: &std::task::Waker) {
        self.stdout.register(waker);
        self.stdout_empty();
    }

    /// A read of the agent's stdout found it empty.
    fn stdout_empty(&self) {
        self.waits.fetch_add(1, Ordering::AcqRel);
        self.idle.store(true, Ordering::Release);
    }

    /// A read of the agent's stdout gave bytes (or an error, which ends the stream next).
    fn stdout_read(&self) {
        // Checked first, so the hot path only reads.
        if self.idle.load(Ordering::Relaxed) {
            self.idle.store(false, Ordering::Release);
        }
    }

    /// The agent's stdout ended, or its reader is gone: nothing more is read.
    fn stdout_ended(&self) {
        self.stdout_empty();
    }

    /// Ready when no event waits for room and fewer than [`MAX_LINES_IN_FLIGHT`] counted lines
    /// wait for the handler.
    fn poll_open(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.is_open() {
            return Poll::Ready(());
        }
        self.reader.register(cx.waker());
        if self.is_open() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// A point in the line reader's progress, from [`ReadProgress::mark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadMark(u64);

/// What the line reader has read of the agent's stdout, for a host that must know whether all
/// the agent wrote before it exited has reached the event channel. From
/// [`AcpClient::read_progress`](crate::AcpClient::read_progress); holding it keeps no part of
/// the connection alive.
#[derive(Debug, Clone)]
pub struct ReadProgress(pub(crate) Arc<Gate>);

impl ReadProgress {
    /// The reader's progress now. Taken once the agent has exited, everything it wrote is in
    /// its stdout pipe or already read. A read that waits is woken to read again, so a reader
    /// that found the pipe empty before the mark finds it so again after it (or reads what came
    /// since) once the [`Driver`](crate::Driver) runs, and nothing else has to wake it.
    pub fn mark(&self) -> ReadMark {
        let mark = ReadMark(self.0.waits.load(Ordering::Acquire));
        self.0.stdout.wake();
        mark
    }

    /// Whether everything in the agent's stdout at `mark` has been read and every
    /// `session/update` and malformed line of it is in the event channel: since `mark`, a read
    /// found the stdout empty with no read after it, or the stdout ended; and no such line waits
    /// between the reader and the event channel. Read it on the thread that runs the
    /// [`Driver`](crate::Driver), so the reader does not move while it is read.
    ///
    /// Exact where a read waits only on an empty pipe, as `async-process`'s stdout does on
    /// Unix. On Windows that stdout is read by a thread, so a read can wait while the thread
    /// still holds bytes, and this can say `true` early.
    ///
    /// Only `session/update`s and malformed lines are counted ([`Gate`]): an agent request,
    /// response or other notification read but not yet handled does not hold this back. A host
    /// waits for a call's answer separately.
    pub fn drained_since(&self, mark: ReadMark) -> bool {
        let gate = &self.0;
        gate.idle.load(Ordering::Acquire)
            && gate.waits.load(Ordering::Acquire) > mark.0
            && gate.in_flight.load(Ordering::Acquire) == 0
            && gate.blocked.load(Ordering::Acquire) == 0
    }
}

/// The agent's stdout as the [`LineSplitter`] reads it, telling the [`Gate`] about each read
/// that finds it empty, each that gives bytes, and its end (or this reader's drop).
struct Watched<R> {
    reader: R,
    gate: Arc<Gate>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Watched<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let read = Pin::new(&mut this.reader).poll_read(cx, buf);
        match &read {
            Poll::Pending => this.gate.stdout_waits(cx.waker()),
            Poll::Ready(Ok(0)) if !buf.is_empty() => this.gate.stdout_ended(),
            Poll::Ready(_) => this.gate.stdout_read(),
        }
        read
    }
}

impl<R> Drop for Watched<R> {
    fn drop(&mut self) {
        self.gate.stdout_ended();
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

/// The lines the SDK's `Lines` transport reads, each copied to the frame tap. A UTF-8 JSON-RPC
/// message that the SDK's decode reads the same way passes as it is ([`Envelope::is_message`]).
/// Blank lines are skipped; any other line, an agent's line naming [`MALFORMED_LINE_METHOD`]
/// included, becomes a [`MALFORMED_LINE_METHOD`] notification, which the inbound handler turns
/// into a [`ProtocolWarning::MalformedLine`](crate::ProtocolWarning::MalformedLine). A line
/// over 8 MiB ends the stream with an error, which closes the connection (the driver reports
/// `Error::Protocol`). Before each line it waits for the [`Gate`].
pub(crate) fn incoming<R>(
    reader: R,
    shared: Arc<Shared>,
) -> impl Stream<Item = io::Result<String>> + Send + 'static
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let reader = Watched {
        reader,
        gate: Arc::clone(&shared.gate),
    };
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
                Ok(line) => {
                    shared.tap(FrameDirection::In, &line);
                    match serde_json::from_str::<Envelope>(&line) {
                        Ok(envelope) if envelope.is_message() => {
                            if envelope.is_update() {
                                shared.gate.line_read();
                            }
                            line
                        }
                        Ok(_) | Err(_) => {
                            shared.gate.line_read();
                            malformed_marker(line.len())
                        }
                    }
                }
                Err(error) => {
                    shared.tap(
                        FrameDirection::In,
                        &String::from_utf8_lossy(error.as_bytes()),
                    );
                    shared.gate.line_read();
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

/// Whether the inbound handler counts this notification off the [`Gate`]: the same lines
/// [`incoming`] counted, a `session/update` whose `params` is an object ([`Envelope::is_update`])
/// or a malformed-line marker, which only [`incoming`] writes (it turns an agent's line naming
/// that method into a marker of its own). The SDK hands each of these to the handler exactly
/// once (see [`Gate`] for when that holds).
pub(crate) fn counted(method: &str, params: &serde_json::Value) -> bool {
    (method == "session/update" || method == MALFORMED_LINE_METHOD) && params.is_object()
}

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
