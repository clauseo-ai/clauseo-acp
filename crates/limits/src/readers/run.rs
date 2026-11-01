//! One run of a reader's program: launch it, talk to it over stdin and stdout while its stderr
//! drains and its exit is awaited, all under one deadline. Claude's reader reads one line; the
//! Codex reader (later) talks JSON-RPC over the same run.

use std::future::Future;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{AgentStdin, LaunchSpec, Launched, Launcher, Timer};
use futures::future::{Either, join3, select};
use futures::{AsyncRead, AsyncReadExt as _};

/// The most bytes a reader takes for its one line; a longer line is malformed.
pub(crate) const LINE_MAX: usize = 1024 * 1024;
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
}

/// Launches `spec` and runs `talk` on its stdin and stdout, beside the stderr pump and the wait
/// for its exit, all racing `timer.after(limit)`. The stderr tail is read only once the exit is
/// in and the pump has ended (`process`'s order), so it holds the last lines.
pub(crate) async fn run<T, F, Fut>(
    launcher: &dyn Launcher,
    spec: &LaunchSpec,
    timer: &T,
    limit: Duration,
    talk: F,
) -> Ran<Fut::Output>
where
    T: Timer,
    F: FnOnce(AgentStdin, Stdout) -> Fut,
    Fut: Future,
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
            let (talked, drained, report) = join3(
                talk(stdin, Box::new(stdout)),
                stderr.drain(),
                handle.exited(),
            )
            .await;
            if let Err(err) = drained {
                tracing::debug!(error = %err, "a reader's stderr ended with an error");
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
    // tree.
    ran.unwrap_or_else(|| {
        drop(handle);
        Ran::TimedOut
    })
}

/// The first line of stdout.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// The line, without its `\n`.
    Read(Vec<u8>),
    /// The line ran past [`LINE_MAX`] bytes.
    TooLong,
    /// Stdout ended (or failed) before a `\n`.
    Missing,
}

/// Reads stdout up to its first `\n`, keeping at most [`LINE_MAX`] bytes, then reads it to its
/// end and drops the rest, so the program never blocks on a full pipe.
pub(crate) async fn first_line(mut stdout: Stdout) -> Line {
    let mut chunk = vec![0; READ];
    let mut line = Vec::new();
    let found = loop {
        let n = match stdout.read(&mut chunk).await {
            Ok(0) | Err(_) => return Line::Missing,
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
    while let Ok(1..) = stdout.read(&mut chunk).await {}
    found
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use futures::io::Cursor;

    use super::*;

    fn line_of(bytes: Vec<u8>) -> Line {
        block_on(first_line(Box::new(Cursor::new(bytes))))
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
