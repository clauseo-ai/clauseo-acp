//! The proxy's credential, supplied to Chrome over CDP (decision records 0029, 0053 and 0060).
//!
//! The session's egress proxy answers 407 to a request without `Proxy-Authorization: Basic
//! base64("clauseo:<password>")`, and Chrome ignores credentials in `--proxy-server`. So the
//! guard answers Chrome's proxy challenge itself, as one of its own clients on the browser's
//! `DevTools` pipe (`devtools/seam.rs`), on the browser's root session:
//! `Fetch.enable {"patterns":[{"urlPattern":"*"}],"handleAuthRequests":true}` pauses every
//! request (the guard continues each at once) and turns the proxy's 407 into
//! `Fetch.authRequired`, answered with the credential when the challenge is the proxy's and
//! cancelled otherwise. Chrome then caches the credential per profile. A pattern matching
//! nothing gives no `authRequired`, and empty patterns are refused with `handleAuthRequests`;
//! per-page interception would need `Target.setAutoAttach`, whose root-session state
//! agent-browser owns (each measured on Chrome 151).
//!
//! The password comes from the file `CLAUSEO_PROXY_SECRET_FILE` names (`env.rs`), which the
//! guard reads once at its start and then deletes; no child sees the variable or the file's
//! content. [`Password`] never prints it; it leaves the guard only inside
//! `Fetch.continueWithAuth`. Both forms of it, the hex and the `Proxy-Authorization` value, are
//! taken out of everything the guard relays or notes ([`hide`], `relay/redact.rs`).
//!
//! [`ProxyAuth`] is a [`GuardClient`]: `seam::clients` makes one for each browser when
//! agent-browser's environment has `AGENT_BROWSER_PROXY` and the guard read a password. The
//! relay sends its `Fetch.enable` and waits for the reply before agent-browser starts, and a
//! refusal fails the browser's start. The relay hands it the root `Fetch` events, which are the
//! guard's (agent-browser's root-session `Fetch` commands are refused, `devtools/routes.rs`),
//! and the replies to its own commands, each sent with an id from `seam::next_guard_id`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::devtools::seam::{CdpLink, GuardClient, next_guard_id, session_of};
use super::relay::redact::{self, Secrets};

/// The user name the proxy expects.
pub(crate) const USERNAME: &str = "clauseo";
/// The event of a paused request.
const REQUEST_PAUSED: &str = "Fetch.requestPaused";
/// The event of an authentication challenge.
const AUTH_REQUIRED: &str = "Fetch.authRequired";
/// The events the responder consumes, on the root session only.
const EVENTS: &[&str] = &[REQUEST_PAUSED, AUTH_REQUIRED];

/// The proxy's password: exactly 64 of `0-9 a-f`. Never printed, `Debug` included.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Password(String);

impl Password {
    /// `text` when it is 64 of `0-9 a-f`.
    pub(crate) fn parse(text: &str) -> Option<Password> {
        let valid =
            text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        valid.then(|| Password(text.to_owned()))
    }

    /// The `Proxy-Authorization` credential Chrome sends: base64 of `clauseo:<password>`.
    fn basic(&self) -> String {
        super::devtools::base64(format!("{USERNAME}:{}", self.0).as_bytes())
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<hidden>)")
    }
}

/// Has the redactor take both forms of `password` out of all the guard relays and notes from
/// now on: the hex (and a long enough start of it), and the base64 credential, whole or a start
/// of it long enough past the base64 of `clauseo:`, which every Basic value for the user shares.
pub(crate) fn hide(password: &Password) {
    let user = super::devtools::base64(format!("{USERNAME}:").as_bytes()).len();
    redact::remember(&Secrets::literal(password.0.as_bytes()));
    redact::remember(&Secrets::literal(password.basic().as_bytes()).start_after(user));
}

/// The responder: answers Chrome's proxy challenge on the root session. See the module header.
pub(crate) struct ProxyAuth {
    /// The browser's pipe, through the relay.
    link: Arc<dyn CdpLink>,
    /// `http://127.0.0.1:<port>`, compared exactly with a challenge's origin.
    origin: String,
    password: Password,
    /// Set once an error reply to a continue was noted: only the first is, so a page cannot
    /// flood the guard's stderr.
    refusal_noted: AtomicBool,
}

impl fmt::Debug for ProxyAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyAuth")
            .field("origin", &self.origin)
            .field("password", &self.password)
            .finish_non_exhaustive()
    }
}

impl ProxyAuth {
    /// A responder for the proxy at `proxy_origin`, exactly `AGENT_BROWSER_PROXY`
    /// (`http://127.0.0.1:<port>`), answering through `link`.
    pub(crate) fn new(link: Arc<dyn CdpLink>, proxy_origin: String, password: Password) -> Self {
        ProxyAuth {
            link,
            origin: proxy_origin,
            password,
            refusal_noted: AtomicBool::new(false),
        }
    }
}

impl GuardClient for ProxyAuth {
    fn events(&self) -> &[&str] {
        EVENTS
    }

    fn start_commands(&self) -> Vec<(String, Value)> {
        vec![(
            "Fetch.enable".to_owned(),
            json!({"patterns": [{"urlPattern": "*"}], "handleAuthRequests": true}),
        )]
    }

    /// A reply to one of its continues: the first error reply is noted, by its code only;
    /// later ones are not.
    fn on_reply(&self, _id: i64, message: &Value) {
        if let Some(error) = message.get("error")
            && !self.refusal_noted.swap(true, Ordering::SeqCst)
        {
            super::note(&format!(
                "Chrome refused a request the guard continued (CDP error {}); later refusals \
                 go unnoted",
                code_of(error)
            ));
        }
    }

    /// A root event (no `sessionId`, or an empty or `null` one, as `seam::session_of` reads
    /// it): a paused request is continued; a challenge is answered with the credential when it
    /// is the proxy's, else cancelled. Anything else is ignored.
    fn on_event(&self, message: &Value) {
        if !session_of(message.get("sessionId")).is_empty() {
            return;
        }
        let params = message.get("params");
        let Some(request_id) = params
            .and_then(|params| params.get("requestId"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let (method, params) = match message.get("method").and_then(Value::as_str) {
            Some(REQUEST_PAUSED) => ("Fetch.continueRequest", json!({"requestId": request_id})),
            Some(AUTH_REQUIRED) => {
                let challenge = params.and_then(|params| params.get("authChallenge"));
                let field =
                    |name: &str| challenge.and_then(|c| c.get(name)).and_then(Value::as_str);
                let response = if field("source") == Some("Proxy")
                    && field("origin") == Some(self.origin.as_str())
                {
                    json!({
                        "response": "ProvideCredentials",
                        "username": USERNAME,
                        "password": self.password.0,
                    })
                } else {
                    json!({"response": "CancelAuth"})
                };
                (
                    "Fetch.continueWithAuth",
                    json!({"requestId": request_id, "authChallengeResponse": response}),
                )
            }
            _ => return,
        };
        let command = json!({"id": next_guard_id(), "method": method, "params": params});
        // A send that fails is a browser gone, which the relay notices and the session tells.
        drop(self.link.send(&command));
    }
}

/// A CDP error's `code`, or 0.
fn code_of(error: &Value) -> i64 {
    error.get("code").and_then(Value::as_i64).unwrap_or(0)
}

#[cfg(test)]
mod tests;
