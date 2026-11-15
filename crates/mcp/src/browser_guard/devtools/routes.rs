//! Which side each CDP message goes to, by the seam's contract (`seam.rs`).
//!
//! - **agent-browser's commands** must carry an integer id from 0 to 2^30 - 1. Any other numeric
//!   id (a guard id, a negative one, a fraction) is answered here with a CDP error and never
//!   reaches the browser; a message with no id, a non-numeric one, or that is no JSON object is
//!   dropped. What goes to the browser is the message as parsed and written again, so a
//!   duplicate `"id"` key cannot mean one thing here and another there. A forwarded
//!   `<Domain>.enable` or `.disable` is noted per session (when forwarded, not when the browser
//!   confirms it: a refused enable brings no events anyway).
//! - **Replies** with a guard id go to the relay's own waiting call if it sent the command, else
//!   to the guard's clients; never to agent-browser. Every other reply goes to agent-browser as
//!   the browser wrote it.
//! - **Events:** a root-session event of a method the clients take goes to them, and to
//!   agent-browser only while it has that domain enabled on the root session. Every other event
//!   goes to agent-browser.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::seam::{FIRST_ID, is_guard_id};

/// What a reply to the relay's own command carries: the browser's `result`, or its `error`.
pub(crate) type Reply = Result<Value, Value>;

/// The routing state, shared by the pipe's reader, the client's reader and the guard's calls.
#[derive(Default)]
pub(crate) struct Routes {
    /// The relay's own commands waiting for their reply.
    pending: BTreeMap<i64, Sender<Reply>>,
    /// The event methods the guard's clients take, from the root session.
    methods: BTreeSet<String>,
    /// The (session id, domain) pairs agent-browser has enabled; `""` is the root session.
    enabled: BTreeSet<(String, String)>,
}

/// What to do with a message agent-browser sent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FromClient {
    /// Write this to the browser.
    Forward(String),
    /// Answer agent-browser with this, and send nothing to the browser.
    Refuse(String),
    /// Send nothing anywhere.
    Drop,
}

/// What to do with a message the browser sent.
#[derive(Debug, PartialEq)]
pub(crate) struct FromBrowser {
    /// Whether agent-browser gets it, as the browser wrote it.
    pub(crate) forward: bool,
    /// What the guard's clients get.
    pub(crate) guard: ToGuard,
}

/// What the guard's clients get of a browser message.
#[derive(Debug, PartialEq)]
pub(crate) enum ToGuard {
    Nothing,
    /// A reply to a guard id the relay did not send: `on_reply`.
    Reply(i64, Value),
    /// A root-session event of a method they take: `on_event`.
    Event(Value),
}

impl FromBrowser {
    fn forward() -> Self {
        FromBrowser {
            forward: true,
            guard: ToGuard::Nothing,
        }
    }
}

impl Routes {
    /// Notes that the reply to the relay's own command `id` goes to `reply`.
    pub(crate) fn await_reply(&mut self, id: i64, reply: Sender<Reply>) {
        self.pending.insert(id, reply);
    }

    /// Forgets a command of the relay's that gave up waiting.
    pub(crate) fn forget(&mut self, id: i64) {
        self.pending.remove(&id);
    }

    /// The guard's clients take root-session events of `methods` from now on.
    pub(crate) fn take_events<'a>(&mut self, methods: impl IntoIterator<Item = &'a str>) {
        self.methods.extend(methods.into_iter().map(str::to_owned));
    }

    /// Judges a message from agent-browser, noting a domain it enables or disables.
    pub(crate) fn client_message(&mut self, text: &str) -> FromClient {
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(text) else {
            return FromClient::Drop;
        };
        let Some(id) = message.get("id") else {
            return FromClient::Drop;
        };
        let allowed = id.as_i64().is_some_and(|id| (0..FIRST_ID).contains(&id));
        if !allowed {
            if !id.is_number() {
                return FromClient::Drop;
            }
            let refusal = json!({
                "id": id,
                "error": {"code": -32600, "message": "the id is outside agent-browser's range"},
            });
            return FromClient::Refuse(refusal.to_string());
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let session = session_of(message.get("sessionId"));
            if let Some(domain) = method.strip_suffix(".enable") {
                self.enabled.insert((session, domain.to_owned()));
            } else if let Some(domain) = method.strip_suffix(".disable") {
                self.enabled.remove(&(session, domain.to_owned()));
            }
        }
        FromClient::Forward(Value::Object(message).to_string())
    }

    /// Routes a message from the browser. The relay's own replies are delivered here.
    pub(crate) fn browser_message(&mut self, text: &[u8]) -> FromBrowser {
        let Ok(Value::Object(mut message)) = serde_json::from_slice::<Value>(text) else {
            return FromBrowser::forward();
        };
        if let Some(id) = message.get("id").and_then(Value::as_i64) {
            if !is_guard_id(id) {
                return FromBrowser::forward();
            }
            let guard = match self.pending.remove(&id) {
                Some(reply) => {
                    let outcome = match message.remove("error") {
                        Some(error) => Err(error),
                        None => Ok(message.remove("result").unwrap_or(Value::Null)),
                    };
                    // A call that gave up has dropped its receiver.
                    drop(reply.send(outcome));
                    ToGuard::Nothing
                }
                None => ToGuard::Reply(id, Value::Object(message)),
            };
            return FromBrowser {
                forward: false,
                guard,
            };
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return FromBrowser::forward();
        };
        if message.contains_key("sessionId") || !self.methods.contains(method) {
            return FromBrowser::forward();
        }
        let domain = method.split_once('.').map_or("", |(domain, _)| domain);
        let forward = self.enabled.contains(&(String::new(), domain.to_owned()));
        FromBrowser {
            forward,
            guard: ToGuard::Event(Value::Object(message)),
        }
    }
}

/// A message's `sessionId`, or `""` for the root session.
fn session_of(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}
