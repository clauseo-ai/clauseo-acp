//! The outer runner's end of the denial pipe: opened before anything runs as the pool user
//! ([`open_feed`]), then records queued by the git watch and written by a thread of their own,
//! so a host that reads slowly never holds up a watch (whose kill must not wait). With no pipe
//! named, a record is only logged: never written to stderr, where the agent's own lines go too.

use std::io::{self, Write};
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use super::{Record, encode};

/// What became of a record handed to [`RecordSink::send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sent {
    /// Queued for the pipe.
    Queued,
    /// Logged only: no pipe, or it failed, or the sink was drained.
    Logged,
}

/// The records' queue and the thread that writes them.
pub(crate) struct RecordSink {
    queue: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    done: Mutex<Option<mpsc::Receiver<()>>>,
}

impl RecordSink {
    /// A sink writing to `pipe`, one record a line; `None` logs each record only.
    pub(crate) fn start(pipe: Option<Box<dyn Write + Send>>) -> RecordSink {
        let Some(mut pipe) = pipe else {
            return RecordSink::logging();
        };
        let (queue, lines) = mpsc::channel::<Vec<u8>>();
        let (finished, done) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("denial-pipe".to_owned())
            .spawn(move || {
                for line in &lines {
                    if let Err(error) = pipe.write_all(&line).and_then(|()| pipe.flush()) {
                        // The queue closes with this thread: later records are only logged.
                        let lost = lines.try_iter().count().saturating_add(1);
                        tracing::warn!(%error, lost, "the denial pipe failed; records are logged");
                        break;
                    }
                }
                drop(lines);
                let _sent = finished.send(());
            });
        match spawned {
            Ok(_thread) => RecordSink {
                queue: Mutex::new(Some(queue)),
                done: Mutex::new(Some(done)),
            },
            Err(error) => {
                tracing::warn!(%error, "the denial pipe's thread couldn't start");
                RecordSink::logging()
            }
        }
    }

    /// A sink with no pipe.
    fn logging() -> RecordSink {
        RecordSink {
            queue: Mutex::new(None),
            done: Mutex::new(None),
        }
    }

    /// Queues `record` for the pipe, or logs it when there is none.
    pub(crate) fn send(&self, record: &Record) -> Sent {
        let line = match encode(record) {
            Ok(line) => line,
            Err(error) => {
                tracing::warn!(kind = record.kind(), %error, "a record couldn't be encoded");
                return Sent::Logged;
            }
        };
        let queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if queue.as_ref().is_some_and(|queue| queue.send(line).is_ok()) {
            return Sent::Queued;
        }
        tracing::warn!(
            kind = record.kind(),
            "no denial pipe: the git watch's record is only logged"
        );
        Sent::Logged
    }

    /// Closes the queue and waits up to `wait` for what was queued to be written.
    pub(crate) fn drain(&self, wait: Duration) {
        drop(
            self.queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
        let done = self
            .done
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if done.is_some_and(|done| done.recv_timeout(wait).is_err()) {
            tracing::warn!("the denial pipe was still busy at the session's end");
        }
    }
}

/// `ERROR_FILE_NOT_FOUND`: no pipe has the name (the host's end is gone).
const NOT_FOUND: i32 = 2;
/// `ERROR_PIPE_BUSY`: the pipe's one instance is taken.
const BUSY: i32 = 231;

/// The outer's sink for the launch's denial pipe `name`, opened with `open`. No name: a sink
/// that only logs. A named pipe that can't be opened refuses the launch, with the lawyer's words
/// for why ([`refusal`]): the session never runs without its feed.
pub(crate) fn open_feed<W: Write + Send + 'static>(
    name: Option<&str>,
    open: impl FnOnce(&str) -> io::Result<W>,
) -> Result<RecordSink, String> {
    let Some(name) = name else {
        tracing::warn!("the launch names no denial pipe: the git watch's records are only logged");
        return Ok(RecordSink::start(None));
    };
    match open(name) {
        Ok(pipe) => Ok(RecordSink::start(Some(Box::new(pipe)))),
        Err(error) => {
            tracing::warn!(%error, "the denial pipe can't be opened: the launch is refused");
            Err(refusal(&error))
        }
    }
}

/// Why the session didn't start when its denial pipe couldn't be opened, in plain words.
pub(crate) fn refusal(error: &io::Error) -> String {
    let why = match error.raw_os_error() {
        Some(BUSY) => "another program was already connected to it".to_owned(),
        Some(NOT_FOUND) => "it had already closed".to_owned(),
        Some(code) => format!("it couldn't be reached (error {code})"),
        None => format!("it couldn't be reached ({})", error.kind()),
    };
    format!(
        "Clauseo didn't start the session: it couldn't open the channel it uses to tell you what \
         it moved aside, because {why}. Try again; if this keeps happening, restart Clauseo."
    )
}
