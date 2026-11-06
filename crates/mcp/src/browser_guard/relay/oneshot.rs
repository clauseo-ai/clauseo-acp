//! The answer to a request while the browser is stopped: a one-shot agent-browser.
//!
//! agent-browser's MCP server keeps no state between messages: `initialize`, `ping`,
//! `tools/list` and `shutdown` are answered from the tool table and the request alone, an
//! unknown method with an error, and nothing but `tools/call` runs the CLI, its daemon or a
//! browser (v0.41.1 `mcp.rs:661-725`). So while the browser is stopped the guard answers any
//! request but a tool call by starting agent-browser with its arguments and environment, less
//! `AGENT_BROWSER_CDP` (no Chrome exists), writing the request, closing its stdin and relaying
//! its whole lines until it exits; the next start of the running agent-browser needs no replayed
//! handshake. The answer is agent-browser's own, so no copy of its tool list can drift from it.
//!
//! It runs as the running one does: its own process group (Unix) or the guard's job (Windows),
//! told to the reaper, and killed whole once it ends. It has [`ONESHOT_WAIT`] to exit; then the
//! guard kills it. A one-shot that exits with a code other than 0, or after a `shutdown`, ends
//! the guard with that code, as the running agent-browser's exit does. None starts once the
//! relay has ended (`State::ended`): the request gets an error instead.

use std::io::{self, BufReader, Write};
use std::process::ExitStatus;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use super::super::session::{self, Shared};
use serde_json::Value;

use super::lines::{Line, Overflow, answer_all, emit, lock, read_line};
use super::{CHILD_MAX_LINE_BYTES, ENDED, Event, end_group, pumps, send};
use crate::browser_guard::note;

/// How long a one-shot agent-browser has to answer and exit.
pub(crate) const ONESHOT_WAIT: Duration = Duration::from_secs(10);

/// What the one-shot did.
enum Once {
    /// Nothing: the relay had ended (`State::ended`), so none started.
    Ended,
    /// It exited with this status.
    Exited(ExitStatus),
    /// It wrote a line over [`CHILD_MAX_LINE_BYTES`] and was killed.
    TooLong,
}

/// Answers request `text` with a one-shot agent-browser. `false` when the guard ends: the
/// one-shot exited with a code other than 0, or after `shutdown`, or wrote an oversize line, or
/// could not start; the event that says so is sent. Once the relay has ended, request `id` gets
/// an error and no one-shot starts.
pub(crate) fn answer(
    shared: &Arc<Shared>,
    events: &mpsc::Sender<Event>,
    text: &str,
    id: &Value,
    shutdown: bool,
) -> bool {
    match run_once(shared, text) {
        Ok(Once::Ended) => {
            answer_all(&shared.out, std::slice::from_ref(id), ENDED);
            return true;
        }
        Ok(Once::Exited(status)) => {
            let code = status.code().unwrap_or(1);
            if code == 0 && !shutdown {
                return true;
            }
            send(events, Event::OneShotEnded(code));
        }
        Ok(Once::TooLong) => send(events, Event::OneShotTooLong),
        Err(error) => send(events, Event::AgentFailed(error.to_string())),
    }
    false
}

/// Starts the one-shot, relays its lines and reaps it. It starts, and is stored where
/// [`end`] finds it, under the state's lock and only if the relay has not ended, so the relay's
/// end either sees it or keeps it from starting.
fn run_once(shared: &Arc<Shared>, text: &str) -> io::Result<Once> {
    let launcher = &shared.launcher;
    let (stdin, stdout) = {
        let state = lock(&shared.state);
        if state.ended {
            return Ok(Once::Ended);
        }
        let mut child = pumps::spawn_child(
            &launcher.program,
            &launcher.args,
            &launcher.cwd,
            &launcher.env,
        )?;
        #[cfg(windows)]
        if let Err(error) = launcher.job.adopt(&mut child, "agent-browser") {
            drop(child.wait());
            return Err(error);
        }
        #[cfg(unix)]
        launcher.reaper.say(&format!("agent {}", child.id()));
        let pipes = (child.stdin.take(), child.stdout.take());
        *lock(&shared.oneshot) = Some(child);
        drop(state);
        pipes
    };
    let (done, finished) = mpsc::channel::<()>();
    {
        let shared = Arc::clone(shared);
        drop(thread::spawn(move || {
            if finished.recv_timeout(ONESHOT_WAIT) == Err(mpsc::RecvTimeoutError::Timeout)
                && let Some(child) = lock(&shared.oneshot).as_mut()
            {
                note(&format!(
                    "agent-browser did not answer within {} s with no browser; killed it",
                    ONESHOT_WAIT.as_secs()
                ));
                end_group(child);
            }
        }));
    }
    if let Some(mut stdin) = stdin {
        // A one-shot that is gone already shows in its exit below. Dropping stdin closes it.
        drop(writeln!(stdin, "{text}").and_then(|()| stdin.flush()));
    }
    let mut too_long = false;
    if let Some(stdout) = stdout {
        let mut input = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            match read_line(&mut input, &mut line, CHILD_MAX_LINE_BYTES, Overflow::Stop) {
                Ok(Line::Whole) => {
                    line.push(b'\n');
                    // An agent that stopped reading: its stdin's end follows.
                    drop(emit(&shared.out, &line));
                }
                Ok(Line::Partial) => {
                    note(&format!(
                        "agent-browser ended in the middle of a line; dropped its last {} bytes",
                        line.len()
                    ));
                    break;
                }
                Ok(Line::Oversize) => {
                    too_long = true;
                    break;
                }
                Ok(Line::Eof) | Err(_) => break,
            }
        }
    }
    drop(done);
    let status = end(shared).unwrap_or_else(|| Err(io::Error::other("the one-shot was reaped")))?;
    Ok(if too_long {
        Once::TooLong
    } else {
        Once::Exited(status)
    })
}

/// Kills the one-shot's group, if one runs, and reaps it: after its answer, and when the guard
/// ends.
pub(crate) fn end(shared: &Shared) -> Option<io::Result<ExitStatus>> {
    let mut oneshot = lock(&shared.oneshot);
    let mut child = oneshot.take()?;
    Some(session::end_agent(&shared.launcher, &mut child))
}
