//! The helper's `DevTools` client: CDP over the two pipes chrome was given
//! (`--remote-debugging-pipe`), JSON messages each ended by one NUL byte in both directions. A
//! reader thread splits chrome's replies and events on NUL and hands them over a channel; every
//! wait is bounded by the tick clock. Each step prints a `cdp` line; a step that fails prints
//! `cdp method=<m> failed: <why>` (CDP's error message, which holds no SID) and stops the helper.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::helper::say;
use super::win::{Res, tick_ms};

/// A CDP connection to chrome.
pub(crate) struct Cdp {
    /// Commands to chrome.
    writer: PipeWriter,
    /// Chrome's messages, parsed; the channel closes when chrome's pipe does.
    messages: Receiver<Result<Value, String>>,
    /// Events read while waiting for something else.
    events: VecDeque<Value>,
    /// The next command's id.
    next: u64,
}

/// What a wait for a reply ended with.
pub(crate) enum Reply {
    /// The result.
    Result(Value),
    /// Chrome's pipe closed first.
    Closed,
}

impl Cdp {
    /// Starts the reader thread over `reader`.
    pub(crate) fn start(writer: PipeWriter, reader: PipeReader) -> Cdp {
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            for chunk in BufReader::new(reader).split(0) {
                let parsed = match chunk {
                    Ok(bytes) if bytes.is_empty() => continue,
                    Ok(bytes) => serde_json::from_slice(&bytes)
                        .map_err(|e| format!("a message from chrome is not JSON: {e}")),
                    Err(e) => Err(format!("reading chrome's pipe: {e}")),
                };
                let failed = parsed.is_err();
                if sender.send(parsed).is_err() || failed {
                    break;
                }
            }
        });
        Cdp {
            writer,
            messages,
            events: VecDeque::new(),
            next: 1,
        }
    }

    /// Sends `method` with `params` (to `session`, when given); its id.
    pub(crate) fn send(&mut self, method: &str, params: Value, session: Option<&str>) -> Res<u64> {
        let id = self.next;
        self.next += 1;
        let mut message = json!({ "id": id, "method": method });
        message["params"] = params;
        if let Some(session) = session {
            message["sessionId"] = Value::from(session);
        }
        let mut bytes = serde_json::to_vec(&message).map_err(|e| format!("{method}: {e}"))?;
        bytes.push(0);
        self.writer
            .write_all(&bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|e| failure(method, &format!("writing the command: {e}")))?;
        Ok(id)
    }

    /// The next message within `deadline` (a tick); `None` at the deadline, `Err` when the pipe
    /// closed or broke.
    fn next_message(&self, deadline: u64) -> Option<Result<Value, String>> {
        let left = deadline.saturating_sub(tick_ms());
        match self.messages.recv_timeout(Duration::from_millis(left)) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err("chrome's pipe closed".to_owned())),
        }
    }

    /// Waits at most `timeout` for the reply to command `id` (`method`), keeping the events
    /// that come first. A CDP error, a timeout or a broken pipe is the failure.
    pub(crate) fn reply(&mut self, id: u64, method: &str, timeout: Duration) -> Res<Reply> {
        let deadline = tick_ms() + millis(timeout);
        loop {
            let message = match self.next_message(deadline) {
                None => {
                    return Err(failure(
                        method,
                        &format!("no reply within {} s", timeout.as_secs()),
                    ));
                }
                Some(Err(e)) if e == "chrome's pipe closed" => return Ok(Reply::Closed),
                Some(Err(e)) => return Err(failure(method, &e)),
                Some(Ok(message)) => message,
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                self.events.push_back(message);
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("an error without a message");
                return Err(failure(method, text));
            }
            return Ok(Reply::Result(
                message.get("result").cloned().unwrap_or(Value::Null),
            ));
        }
    }

    /// `send`, then `reply`; a closed pipe is a failure.
    pub(crate) fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        timeout: Duration,
    ) -> Res<Value> {
        let id = self.send(method, params, session)?;
        match self.reply(id, method, timeout)? {
            Reply::Result(result) => Ok(result),
            Reply::Closed => Err(failure(method, "chrome's pipe closed before the reply")),
        }
    }

    /// Waits at most `timeout` for the event `method` of `session`.
    pub(crate) fn event(&mut self, method: &str, session: &str, timeout: Duration) -> Res<Value> {
        let matches = |m: &Value| {
            m.get("method").and_then(Value::as_str) == Some(method)
                && m.get("sessionId").and_then(Value::as_str) == Some(session)
        };
        if let Some(at) = self.events.iter().position(matches) {
            return Ok(self.events.remove(at).unwrap_or(Value::Null));
        }
        let deadline = tick_ms() + millis(timeout);
        loop {
            match self.next_message(deadline) {
                None => {
                    return Err(failure(
                        method,
                        &format!("not within {} s", timeout.as_secs()),
                    ));
                }
                Some(Err(e)) => return Err(failure(method, &e)),
                Some(Ok(message)) if matches(&message) => return Ok(message),
                Some(Ok(_other)) => {}
            }
        }
    }
}

/// `timeout` in milliseconds.
fn millis(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

/// Prints the step's failure line and returns it as the failure.
pub(crate) fn failure(method: &str, why: &str) -> String {
    let line = format!("cdp method={method} failed: {why}");
    say(&line);
    line
}

/// A string field of `value`, with its spaces made `_` for a report line; `?` when missing.
pub(crate) fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map_or("?".to_owned(), |s| s.replace(char::is_whitespace, "_"))
}
