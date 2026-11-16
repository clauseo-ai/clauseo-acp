//! Which side each CDP message goes to, by the seam's contract (`seam.rs`).
//!
//! - **agent-browser's commands** must carry an integer id from 0 to 2^30 - 1. Any other numeric
//!   id (a guard id, a negative one, a fraction) is answered here with a CDP error and never
//!   reaches the browser; a message with no id, a non-numeric one, or that is no JSON object is
//!   dropped. What goes to the browser is the message as parsed and written again, so a
//!   duplicate `"id"` key cannot mean one thing here and another there. A root-session command
//!   (no `sessionId`, or an empty or `null` one: Chromium takes an empty one for the root) in a
//!   domain the guard's clients take (`Fetch`, while the proxy's `ProxyAuth` is one) is answered
//!   here with a CDP error too: the guard and agent-browser share the root session, so
//!   agent-browser's `Fetch.enable`, `Fetch.disable` or `Fetch.continueWithAuth` there would
//!   cancel or answer for the guard's own. Page-session commands are not judged by domain. A
//!   forwarded `<Domain>.enable` or `.disable` is noted per session (when forwarded, not when
//!   the browser confirms it: a refused enable brings no events anyway); a new client starts
//!   with none noted ([`Routes::client_connected`]).
//! - **Certificate warnings:** a command that would let a page past a certificate error is
//!   answered here with a CDP error, on every session, the root and each page's alike
//!   ([`certificate_bypass`]): `Security.setIgnoreCertificateErrors` unless `ignore` is
//!   `false`, `Security.setOverrideCertificateErrors` unless `override` is `false`,
//!   `Security.handleCertificateError` unless `action` is `"cancel"`, and two commands that would
//!   carry any command past this check: `Target.sendMessageToTarget` (a command inside a string,
//!   for a session agent-browser never uses: it attaches flat) and `Target.exposeDevToolsProtocol`
//!   (a page's own script gets a `DevTools` connection that does not pass through the guard).
//!   Those are all of Chrome for Testing 147's protocol (`/json/protocol`) that touch a
//!   certificate error; it has no command that sets a preference or a flag. The profile's `ssl`
//!   preferences (`quiet.rs`) take the warning's "Proceed" away; these keep `DevTools` from
//!   skipping the warning altogether.
//! - **Replies** with a guard id go to the relay's own waiting call if it sent the command, else
//!   to the guard's clients; never to agent-browser. Every other reply goes to agent-browser as
//!   the browser wrote it.
//! - **Events:** a root-session event of a method the clients take goes to them, and to
//!   agent-browser only while it has that domain enabled on the root session (which the
//!   refusal above leaves it no way to do). Every other event goes to agent-browser.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::seam::{FIRST_ID, is_guard_id, session_of};

/// What a reply to the relay's own command carries: the browser's `result`, or its `error`.
pub(crate) type Reply = Result<Value, Value>;

/// The routing state, shared by the pipe's reader, the client's reader and the guard's calls.
#[derive(Default)]
pub(crate) struct Routes {
    /// The relay's own commands waiting for their reply.
    pending: BTreeMap<i64, Sender<Reply>>,
    /// The event methods the guard's clients take, from the root session.
    methods: BTreeSet<String>,
    /// The domains the guard's clients take on the root session: those of their events and of
    /// their start commands. agent-browser's root-session commands in them are refused.
    domains: BTreeSet<String>,
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

    /// The guard's clients take root-session events of `methods` from now on, and their
    /// domains.
    pub(crate) fn take_events<'a>(&mut self, methods: impl IntoIterator<Item = &'a str>) {
        for method in methods {
            self.domains.insert(domain_of(method).to_owned());
            self.methods.insert(method.to_owned());
        }
    }

    /// The guard's clients take the domains of `methods` (their start commands') on the root
    /// session from now on.
    pub(crate) fn take_domains<'a>(&mut self, methods: impl IntoIterator<Item = &'a str>) {
        self.domains.extend(
            methods
                .into_iter()
                .map(|method| domain_of(method).to_owned()),
        );
    }

    /// A new agent-browser client is connected: nothing it enabled before counts for it.
    pub(crate) fn client_connected(&mut self) {
        self.enabled.clear();
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
            return FromClient::Refuse(refusal(id, "the id is outside agent-browser's range"));
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(why) = certificate_bypass(method, message.get("params")) {
                return FromClient::Refuse(refusal(id, why));
            }
            let session = session_of(message.get("sessionId")).to_owned();
            if session.is_empty() && self.domains.contains(domain_of(method)) {
                return FromClient::Refuse(refusal(
                    id,
                    "the guard takes this domain on the root session",
                ));
            }
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
        if !session_of(message.get("sessionId")).is_empty() || !self.methods.contains(method) {
            return FromBrowser::forward();
        }
        let forward = self
            .enabled
            .contains(&(String::new(), domain_of(method).to_owned()));
        FromBrowser {
            forward,
            guard: ToGuard::Event(Value::Object(message)),
        }
    }
}

/// Why the guard refuses `method` with `params`, on any session, when it would let a page past a
/// certificate error (the top of this file); `None` for every other command. A switch is let
/// through only when it is exactly off: `false`, or the action `"cancel"`; a missing, `null` or
/// mistyped value is refused.
pub(crate) fn certificate_bypass(method: &str, params: Option<&Value>) -> Option<&'static str> {
    let param = |name: &str| params.and_then(|params| params.get(name));
    let off = |name: &str| param(name) == Some(&Value::Bool(false));
    let refused = match method {
        "Security.setIgnoreCertificateErrors" => !off("ignore"),
        "Security.setOverrideCertificateErrors" => !off("override"),
        "Security.handleCertificateError" => {
            param("action").and_then(Value::as_str) != Some("cancel")
        }
        "Target.sendMessageToTarget" | "Target.exposeDevToolsProtocol" => {
            return Some(CARRIES_COMMANDS);
        }
        _ => false,
    };
    refused.then_some(CERTIFICATE_BYPASS)
}

/// The refusal of a command that skips a certificate error.
pub(crate) const CERTIFICATE_BYPASS: &str =
    "Clauseo does not allow skipping a certificate error in the browser";
/// The refusal of a command that would carry other commands past the guard's checks.
pub(crate) const CARRIES_COMMANDS: &str =
    "Clauseo does not allow a DevTools command that carries other commands past its checks";

/// The local CDP error (-32600, as JSON-RPC's invalid request) for agent-browser's command `id`.
fn refusal(id: &Value, message: &str) -> String {
    json!({"id": id, "error": {"code": -32600, "message": message}}).to_string()
}

/// A CDP method's domain: `Fetch` for `Fetch.enable`; the whole name if it has no dot.
fn domain_of(method: &str) -> &str {
    method.split_once('.').map_or(method, |(domain, _)| domain)
}
