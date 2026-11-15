//! The seam for the guard's own CDP traffic, the contract the proxy's credential
//! (`proxy_auth.rs`'s `ProxyAuth`) plugs into (route 2, decision record 0060).
//!
//! - **Ids.** The guard's commands take ids from [`FIRST_ID`] to `i32::MAX` ([`is_guard_id`]),
//!   all from one counter, [`next_guard_id`]. Chromium takes only an `INT32` id
//!   (`crdtp/dispatch.cc`). agent-browser's commands with any other id are refused by the relay
//!   (`routes.rs`).
//! - **The link.** A [`GuardClient`] sends through a [`CdpLink`]: the relay's, on every system
//!   (the guard holds the browser's pipe). A message with no `sessionId` goes to the browser's
//!   root session. `send` may be called from the relay's reader thread, inside
//!   [`GuardClient::on_event`].
//! - **Replies.** Every reply with a guard id goes to the guard only: to the relay's own waiting
//!   call when it sent the command, else to each client's [`GuardClient::on_reply`]. Never to
//!   agent-browser.
//! - **Events.** A client names the methods it takes ([`GuardClient::events`]). A root-session
//!   event (no `sessionId`) of such a method goes to [`GuardClient::on_event`], and to
//!   agent-browser too only while agent-browser itself has that domain enabled on the root
//!   session. Page-session events, and every other event, go to agent-browser as before.
//! - **Domains.** The domains of a client's events and start commands are the guard's on the
//!   root session: agent-browser's root-session commands in them (`Fetch.enable`,
//!   `Fetch.continueWithAuth`) are refused with a CDP error and never reach the browser, so it
//!   cannot cancel or answer for the guard's own. Its page-session commands are not affected.
//! - **Order.** Before agent-browser is started, and so before anything may connect to the
//!   relay, the guard sends each client's [`GuardClient::start_commands`] and waits up to
//!   [`START_COMMAND_WAIT`] for each reply; a refusal or no answer fails the browser's start.
//!   The guard never sends `Target.setAutoAttach`.
//!
//! [`clients`] is the hook: it returns the `ProxyAuth` when agent-browser's environment has
//! `AGENT_BROWSER_PROXY` and the guard read the proxy's password, and no client otherwise. The
//! guard never starts with one and not the other (`env::proxy_secret` refuses it, exit 2).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::Value;

use super::super::env;
use super::super::proxy_auth::{Password, ProxyAuth};

/// The first guard command id: 2^30.
pub(crate) const FIRST_ID: i64 = 1 << 30;
/// How long each start command has for its reply.
pub(crate) const START_COMMAND_WAIT: Duration = Duration::from_secs(5);

/// The guard's one id counter, an offset from [`FIRST_ID`].
static NEXT: AtomicI64 = AtomicI64::new(0);

/// Whether `id` is one of the guard's: from [`FIRST_ID`] to `i32::MAX`.
pub(crate) fn is_guard_id(id: i64) -> bool {
    (FIRST_ID..=i64::from(i32::MAX)).contains(&id)
}

/// The next guard id, wrapping to [`FIRST_ID`] after `i32::MAX`.
pub(crate) fn next_guard_id() -> i64 {
    let span = i64::from(i32::MAX) - FIRST_ID + 1;
    FIRST_ID + NEXT.fetch_add(1, Ordering::Relaxed).rem_euclid(span)
}

/// A message's `sessionId` (its value), or `""` for the browser's root session. Chromium takes
/// a missing or empty one for the root session; a `null` or other non-string one, which it
/// refuses, is judged as the root session here too, so it is refused where the root session's
/// would be (`routes.rs`) and taken as the root's where the guard's clients read events.
pub(crate) fn session_of(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or_default()
}

/// Where the guard's own CDP messages go: the browser's root session unless the message names a
/// `sessionId`.
pub(crate) trait CdpLink: Send + Sync {
    /// Writes `message` to the browser.
    fn send(&self, message: &Value) -> io::Result<()>;
}

/// A user of the guard's CDP link (`ProxyAuth`).
pub(crate) trait GuardClient: Send + Sync {
    /// The event methods it takes, from the root session only.
    fn events(&self) -> &[&str];
    /// The commands (method and params) to send and see answered before agent-browser starts.
    fn start_commands(&self) -> Vec<(String, Value)>;
    /// A reply to a guard id the relay did not send itself.
    fn on_reply(&self, id: i64, message: &Value);
    /// A root-session event of one of [`GuardClient::events`]; runs on the relay's reader thread.
    fn on_event(&self, message: &Value);
}

/// The guard's clients for one browser, given its link, agent-browser's environment (which
/// holds the session's proxy) and the proxy's password, if the guard read one: the
/// `ProxyAuth` for that proxy when there are both, else none (with neither: the guard refuses
/// one without the other at its start).
pub(crate) fn clients(
    link: &Arc<dyn CdpLink>,
    env: &BTreeMap<String, OsString>,
    password: Option<&Password>,
) -> Vec<Arc<dyn GuardClient>> {
    let proxy = env.get(env::PROXY).and_then(|proxy| proxy.to_str());
    let (Some(proxy), Some(password)) = (proxy, password) else {
        return Vec::new();
    };
    let auth: Arc<dyn GuardClient> = Arc::new(ProxyAuth::new(
        Arc::clone(link),
        proxy.to_owned(),
        password.clone(),
    ));
    vec![auth]
}
