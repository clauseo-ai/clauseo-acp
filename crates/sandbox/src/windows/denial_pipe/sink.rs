//! The outer runner's end of the denial pipe: records queued by the git watch and written by a
//! thread of their own, so a host that reads slowly never holds up a watch (whose kill must not
//! wait). With no pipe, a record is only logged: never written to stderr, where the agent's own
//! lines go too.

use std::io::Write;
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
