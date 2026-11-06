//! The two pumps and agent-browser's start. The agent pump reads the agent's lines, runs each
//! through `filter.rs` and routes what passes (`route`); one stdout pump per start of
//! agent-browser copies its lines to the agent, whole lines and unchanged bytes.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};

use serde_json::Value;

use super::super::filter::{self, MAX_LINE_BYTES, RESERVED_ID as CLOSE_ID, Verdict};
use super::super::parent::Parent;
use super::super::port_file::{Expected, Status};
use super::super::session::{self, Shared};
use super::lines::{Line, Overflow, answer_all, emit, lock, read_line, write_child};
use super::{CHILD_MAX_LINE_BYTES, CLOSE_REQUEST, ENDED, Event, oneshot, send};
use crate::browser_guard::note;
use crate::connector::head;

/// The error a tool call gets when the browser cannot start.
const NOT_STARTED: &str = "the browser could not start; the guard is ending the browser server";

#[expect(
    clippy::disallowed_methods,
    reason = "the guard's one fixed child: agent-browser, whose tree the host kills; the host \
              starts the guard itself through the process crate (docs/decisions/0029-browser.md)"
)]
pub(crate) fn spawn_child(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &BTreeMap<String, OsString>,
) -> io::Result<Child> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A group of its own, which `end_group` kills whole.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Suspended until it is in its job (`job.rs`).
        command.creation_flags(super::CREATE_NO_WINDOW | super::super::job::START_SUSPENDED);
    }
    command.spawn()
}

/// Agent to child: every line through the filter, then [`route`]. On EOF, sends the close
/// request to a running agent-browser.
pub(crate) fn pump_agent(
    stdin: &mut impl BufRead,
    shared: &Arc<Shared>,
    parent: &Parent,
    events: &mpsc::Sender<Event>,
) {
    let mut line = Vec::new();
    loop {
        let verdict = match read_line(stdin, &mut line, MAX_LINE_BYTES, Overflow::Skip) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Oversize) => filter::parse_error(),
            // The agent's last line may lack its newline; it is checked like any other.
            Ok(Line::Whole | Line::Partial) => filter::check(&line),
        };
        match verdict {
            Verdict::Forward(text) => {
                if !route(shared, parent, events, &text) {
                    return;
                }
            }
            // The agent has gone if its stdout fails; its stdin EOF follows.
            Verdict::Answer(text) => drop(emit(&shared.out, format!("{text}\n").as_bytes())),
            Verdict::Drop => {}
        }
    }
    shared.closing.store(true, Ordering::SeqCst);
    let to_child = {
        let mut state = lock(&shared.state);
        state.closing = true;
        state.live.as_ref().map(|live| Arc::clone(&live.to_child))
    };
    send(events, Event::AgentEnded);
    if let Some(to_child) = to_child
        && write_child(&to_child, CLOSE_REQUEST).is_err()
    {
        send(events, Event::CloseAnswered);
    }
}

/// Sends one checked line on. While the browser runs, to agent-browser, noting a request's id
/// until its reply. While it is stopped, a tool call starts it first; any other message with an
/// id goes to a one-shot agent-browser (`oneshot.rs`), which answers it as the running one would,
/// with no Chrome; a notification is dropped, as agent-browser drops it (`mcp.rs:661-692`).
/// Once the relay has ended (`State::ended`) a request gets an error and nothing starts.
/// `false` once the guard is ending and the pump stops.
fn route(shared: &Arc<Shared>, parent: &Parent, events: &mpsc::Sender<Event>, text: &str) -> bool {
    let message: Value = serde_json::from_str(text).unwrap_or_default();
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let call = id.is_some() && method == Some("tools/call");
    let to_child = {
        let mut state = lock(&shared.state);
        if state.ended {
            drop(state);
            answer_all(&shared.out, &id.into_iter().collect::<Vec<_>>(), ENDED);
            return true;
        }
        if state.live.is_none()
            && call
            && let Err(reason) = session::start(shared, &mut state, parent, events)
        {
            drop(state);
            answer_all(
                &shared.out,
                &id.into_iter().collect::<Vec<_>>(),
                NOT_STARTED,
            );
            send(events, Event::StartFailed(reason));
            return false;
        }
        let to_child = state.live.as_ref().map(|live| Arc::clone(&live.to_child));
        if let (Some(_), Some(id)) = (&to_child, &id) {
            state.sent(id.clone(), call);
        }
        to_child
    };
    match (to_child, id) {
        // A failed write: agent-browser has ended or Chrome died, and the relay answers the call.
        (Some(to_child), _) => drop(write_child(&to_child, text)),
        (None, Some(id)) => {
            return oneshot::answer(shared, events, text, &id, method == Some("shutdown"));
        }
        (None, None) => {}
    }
    true
}

/// Child to agent, for the start of agent-browser numbered `generation`: each whole line as
/// read, but the reply to the close request, and nothing once the guard has ended this start
/// (it answered what was in flight itself). After each line, every one of the port checks.
/// Returns the event that ends it: [`Event::ChildEnded`], [`Event::ChildOversize`] or
/// [`Event::PortRefused`].
pub(crate) fn pump_child(
    mut stdout: impl BufRead,
    shared: &Shared,
    generation: u64,
    events: &mpsc::Sender<Event>,
) -> Event {
    let mut line = Vec::new();
    // Once the agent stops reading, the child's output is still drained so it never blocks.
    let mut agent_reads = true;
    loop {
        match read_line(&mut stdout, &mut line, CHILD_MAX_LINE_BYTES, Overflow::Stop) {
            Ok(Line::Eof) | Err(_) => return Event::ChildEnded(generation),
            // `browser_guard::run` says so on stderr once the child is killed.
            Ok(Line::Oversize) => return Event::ChildOversize(generation),
            Ok(Line::Partial) => {
                note(&format!(
                    "agent-browser ended in the middle of a line; dropped its last {} bytes",
                    line.len()
                ));
                return Event::ChildEnded(generation);
            }
            Ok(Line::Whole) => {}
        }
        if shared.closing.load(Ordering::SeqCst) && is_close_reply(&line) {
            send(events, Event::CloseAnswered);
            continue;
        }
        {
            let mut state = lock(&shared.state);
            if state.generation != generation {
                continue;
            }
            if let Some(id) = reply_id(&line) {
                state.answered(&id);
            }
        }
        line.push(b'\n');
        if agent_reads && emit(&shared.out, &line).is_err() {
            agent_reads = false;
        }
        if let Some(reason) = refusal(&shared.launcher.port_checks) {
            return Event::PortRefused(generation, reason);
        }
    }
}

/// The id of a reply (a message with an id and no method), if `line` is one. Read with the
/// connector's reader, which skips the rest of the line without building it.
fn reply_id(line: &[u8]) -> Option<Value> {
    head::read(line).filter(|head| !head.method)?.id
}

/// The first refusal among `checks`, each read now; none while every file is missing, empty or
/// on its claimed port. Never stops checking: a match now says nothing about the next line.
pub(crate) fn refusal(checks: &[Expected]) -> Option<String> {
    checks.iter().find_map(|check| match check.check() {
        Status::Pending | Status::Matches => None,
        Status::Refused(reason) => Some(reason),
    })
}

fn is_close_reply(line: &[u8]) -> bool {
    reply_id(line).is_some_and(|id| id.as_str() == Some(CLOSE_ID))
}
