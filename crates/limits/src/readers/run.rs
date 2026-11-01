//! One run of a reader's program: launch it, talk to it over stdin and stdout while its stderr
//! drains and its exit is awaited, all under one deadline. Claude's reader reads one line; the
//! Codex reader (later) talks JSON-RPC over the same run.

use std::future::Future;
use std::pin::pin;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::{AgentStdin, LaunchSpec, Launched, Launcher, Timer};
use futures::channel::oneshot;
use futures::future::{Either, join, maybe_done, pending, select};
use futures::{AsyncRead, AsyncReadExt as _};

/// The most bytes a reader takes for its one line; a longer line is malformed.
pub(crate) const LINE_MAX: usize = 1024 * 1024;
/// How long the pipes may stay open after the exit once the talk has its answer.
pub(crate) const GRACE: Duration = Duration::from_secs(1);
/// The most one stdout read takes.
const READ: usize = 16 * 1024;

/// The program's stdout, as the talk sees it.
pub(crate) type Stdout = Box<dyn AsyncRead + Send + Unpin>;

/// How a run ended.
#[derive(Debug)]
pub(crate) enum Ran<R> {
    /// The launcher refused or failed; its error's text.
    LaunchFailed(String),
    /// The deadline passed first; the handle was dropped, which kills the tree.
    TimedOut,
    /// The program exited, its stderr drained and the talk ended.
    Exited {
        /// The exit code; `None` when a signal ended it.
        code: Option<i32>,
        /// The redacted end of its stderr, read after the exit and the pump's end.
        stderr_tail: String,
        /// What the talk returned.
        talked: R,
    },
    /// The talk got its answer, but the program had not exited [`GRACE`] later; the handle was
    /// dropped, which kills the tree, and the answer is kept. There is no exit report, so no
    /// stderr tail.
    Lingered {
        /// What the talk returned.
        talked: R,
    },
}

/// Launches `spec` and runs `talk` on its stdin and stdout, beside the stderr pump and the wait
/// for its exit, all racing `timer.after(limit)`. The talk returns its answer and the stdout it
/// gives back, which is then read to its end and dropped, so the program never blocks on a full
/// pipe. Claude's talk gives back the real stdout. Codex's gives back `io::empty()`: the
/// app-server client's driver owns the real stdout and drops it when the talk ends, which closes
/// it undrained. That is harmless, since the app-server gets `EPIPE` as it exits.
///
/// The stderr tail is read once the exit is in and both pipes have ended (`process`'s order), so
/// it holds the last lines. Once `answered` says the talk has its answer, the exit and the pipe
/// ends get only [`GRACE`] more: a program that lingers, or a descendant that left the group and
/// holds a pipe open, cannot turn a good read into a timeout. With the exit in, the run reports
/// it ([`Ran::Exited`]); without, it drops the handle and keeps the answer ([`Ran::Lingered`]).
/// Without an answer the run waits for all of them up to the deadline.
pub(crate) async fn run<T, F, Fut, R>(
    launcher: &dyn Launcher,
    spec: &LaunchSpec,
    timer: &T,
    limit: Duration,
    talk: F,
    answered: impl Fn(&R) -> bool,
) -> Ran<R>
where
    T: Timer,
    F: FnOnce(AgentStdin, Stdout) -> Fut,
    Fut: Future<Output = (R, Stdout)>,
    R: Default,
{
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = match launcher.launch(spec) {
        Ok(launched) => launched,
        Err(err) => return Ran::LaunchFailed(err.to_string()),
    };
    let ran = {
        let deadline = pin!(timer.after(limit));
        let work = pin!(async {
            // The talk's answer, as soon as it has one; stdout then drains beside the exit wait.
            let answer: Mutex<Option<R>> = Mutex::new(None);
            let (answer_in, answer_signal) = oneshot::channel::<()>();
            let stdout_side = async {
                let (talked, stdout) = talk(stdin, Box::new(stdout)).await;
                *answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(talked);
                answer_in.send(()).unwrap_or_default();
                drain_stdout(stdout).await;
            };
            let stderr_side = async { note_stderr_end(stderr.drain().await) };
            let mut pipes = pin!(async {
                join(stdout_side, stderr_side).await;
            });
            let mut exit = pin!(maybe_done(handle.exited()));
            let signal = pin!(async {
                // A talk dropped before its answer never signals.
                if answer_signal.await.is_err() {
                    pending::<()>().await;
                }
            });
            let everything = join(exit.as_mut(), pipes.as_mut());
            let ended = match select(everything, signal).await {
                Either::Left(_) => true,
                Either::Right(((), everything)) => {
                    let has_answer = answer
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_ref()
                        .is_some_and(&answered);
                    let everything = async {
                        everything.await;
                    };
                    if has_answer {
                        settle(everything, timer, GRACE).await
                    } else {
                        everything.await;
                        true
                    }
                }
            };
            let talked = answer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .unwrap_or_default();
            let Some(report) = exit.as_mut().take_output() else {
                tracing::debug!("a reader lingered after its answer; its tree is killed");
                return Ran::Lingered { talked };
            };
            if !ended {
                tracing::debug!("a reader's pipes stayed open after its exit; answer kept");
            }
            Ran::Exited {
                code: report.code(),
                stderr_tail: report.stderr_tail(),
                talked,
            }
        });
        match select(work, deadline).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    // The talk, the pump and the exit wait are dropped by now; dropping the handle kills the
    // tree (after an exit the tree was killed already, and nothing more is sent).
    ran.unwrap_or_else(|| {
        drop(handle);
        Ran::TimedOut
    })
}

/// Logs a stderr pump that ended with an error.
fn note_stderr_end(drained: std::io::Result<()>) {
    if let Err(err) = drained {
        tracing::debug!(error = %err, "a reader's stderr ended with an error");
    }
}

/// Waits for `work` at most `grace`; returns whether it ended in time.
async fn settle<T: Timer>(work: impl Future<Output = ()>, timer: &T, grace: Duration) -> bool {
    match select(pin!(work), pin!(timer.after(grace))).await {
        Either::Left(((), _)) => true,
        Either::Right(((), _)) => false,
    }
}

/// The first line of stdout.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) enum Line {
    /// The line, without its `\n`.
    Read(Vec<u8>),
    /// The line ran past [`LINE_MAX`] bytes.
    TooLong,
    /// Stdout ended (or failed) before a `\n`.
    #[default]
    Missing,
}

/// Reads stdout up to its first `\n`, keeping at most [`LINE_MAX`] bytes, and gives back the
/// rest of stdout for [`run`] to drain.
pub(crate) async fn first_line(mut stdout: Stdout) -> (Line, Stdout) {
    let mut chunk = vec![0; READ];
    let mut line = Vec::new();
    let found = loop {
        let n = match stdout.read(&mut chunk).await {
            Ok(0) | Err(_) => break Line::Missing,
            Ok(n) => n,
        };
        let read = chunk.get(..n).unwrap_or_default();
        let end = read.iter().position(|&byte| byte == b'\n');
        line.extend_from_slice(read.get(..end.unwrap_or(n)).unwrap_or_default());
        if line.len() > LINE_MAX {
            break Line::TooLong;
        }
        if end.is_some() {
            break Line::Read(line);
        }
    };
    (found, stdout)
}

/// Reads stdout to its end (or its first error) and drops what it reads.
async fn drain_stdout(mut stdout: Stdout) {
    let mut chunk = vec![0; READ];
    while let Ok(1..) = stdout.read(&mut chunk).await {}
}

#[cfg(test)]
mod tests {
    use clauseo_process::ManualTimer;
    use futures::executor::block_on;
    use futures::future::{pending, ready};
    use futures::io::Cursor;

    use super::*;

    fn line_of(bytes: Vec<u8>) -> Line {
        block_on(first_line(Box::new(Cursor::new(bytes)))).0
    }

    #[test]
    fn the_rest_of_stdout_past_the_lines_read_is_given_back_to_drain() {
        let mut bytes = b"one\n".to_vec();
        bytes.resize(READ * 3, b'x');
        let (line, mut rest) = block_on(first_line(Box::new(Cursor::new(bytes))));
        assert_eq!(line, Line::Read(b"one".to_vec()));
        let mut left = Vec::new();
        block_on(rest.read_to_end(&mut left)).unwrap();
        assert_eq!(left, vec![b'x'; READ * 2]);
    }

    #[test]
    fn the_grace_ends_a_wait_on_pipes_that_stay_open() {
        let timer = ManualTimer::new();
        let (ended, ()) = block_on(join(settle(pending(), &timer, GRACE), async {
            assert_eq!(timer.next_armed().await, Duration::from_secs(1));
            assert!(timer.fire());
        }));
        assert!(!ended);
        assert!(block_on(settle(ready(()), &timer, GRACE)));
    }

    #[test]
    fn the_first_line_is_read_and_the_rest_dropped() {
        assert_eq!(
            line_of(b"{\"ok\":true}\nmore\nlines".to_vec()),
            Line::Read(b"{\"ok\":true}".to_vec())
        );
        assert_eq!(line_of(b"\n".to_vec()), Line::Read(Vec::new()));
    }

    #[test]
    fn no_newline_is_a_missing_line() {
        assert_eq!(line_of(Vec::new()), Line::Missing);
        assert_eq!(line_of(b"{\"ok\":true}".to_vec()), Line::Missing);
    }

    #[test]
    fn a_line_past_the_cap_is_too_long_and_one_at_it_is_read() {
        let mut at_cap = vec![b'x'; LINE_MAX];
        at_cap.push(b'\n');
        assert_eq!(line_of(at_cap), Line::Read(vec![b'x'; LINE_MAX]));
        let mut over = vec![b'x'; LINE_MAX + 1];
        over.extend_from_slice(b"\nnext\n");
        assert_eq!(line_of(over), Line::TooLong);
        assert_eq!(line_of(vec![b'x'; 3 * LINE_MAX]), Line::TooLong);
    }
}
