//! One message, on its own thread: parse, vet, count, forward, answer.
//!
//! 1. A line that is not JSON gets the parse error, an array the batch error, any other
//!    non-object `Invalid Request`. A second hello closes the connection.
//! 2. `initialize` and `server/discover` carry `params.capabilities` as `{}`, so no capability
//!    of the agent's can invite a server-to-client request.
//! 3. A `tools/call` (the method exactly that, on the parsed value) needs a string or number
//!    `id` (one without is refused, never forwarded: it would spend with no answer to show)
//!    and a string `params.name`; then a usable credential, an open session, the caps. A
//!    method that only reads as `tools/call` (case, whitespace, compatibility forms) is refused.
//! 4. The body is the parsed value re-serialised, so what the connector reads is what was
//!    counted; the headers are the checked `protocolVersion` and `Mcp-Session-Id`.
//! 5. The one reply value that answers the request (a response with its `id`) is the line,
//!    passed on raw (`reply.rs`); anything else (a server-to-client request, a notification, an
//!    answer to another id) is dropped and logged by kind; an answer over [`MAX_REPLY_BYTES`]
//!    becomes the reply-too-large answer. A notification or the agent's own response gets
//!    nothing back; a request always gets exactly one answer.
//!
//! [`MAX_REPLY_BYTES`]: super::super::limits::MAX_REPLY_BYTES

mod reply;

use clauseo_connectors::{AuthError, Error, McpReply, McpRequestHeaders};
use serde_json::{Map, Value, json};

use super::super::limits::{Cap, Spend, Taken, local_date};
use super::super::{CredentialEnd, RelayEvent, texts};
use super::conn::Conn;
use super::{Phase, Session, lock};

/// What the relay knows of a message once it is vetted.
struct Call {
    /// The request's `id`; `None` for a notification or the agent's own response.
    id: Option<Value>,
    /// The method, for the headers and the log; `None` for the agent's response.
    method: Option<String>,
    /// For a `tools/call`, what it spends.
    spend: Option<Spend>,
    /// For a `tools/call`, what was counted, for a refund.
    taken: Option<Taken>,
}

impl Call {
    fn tools_call(&self) -> bool {
        self.spend.is_some()
    }

    /// The method as the log may show it.
    fn log_method(&self) -> &str {
        log_name(self.method.as_deref())
    }
}

/// Handles one line from the agent.
pub(super) fn message(conn: &Conn, line: &[u8]) {
    let hub = &conn.session.hub;
    let started = hub.clock.now().mono;
    let mut value = match serde_json::from_slice::<Value>(line) {
        Ok(value) => value,
        Err(_not_json) => {
            answer(
                conn,
                &texts::rpc_error(&Value::Null, -32700, texts::PARSE_ERROR),
            );
            return;
        }
    };
    let call = match vet(conn, &mut value) {
        Ok(call) => call,
        Err(Some(line)) => {
            answer(conn, &line);
            return;
        }
        Err(None) => return,
    };
    if conn.session.phase() != Phase::Open {
        // The session is ending; its connections are closing. Nothing went out.
        refund(&conn.session, &call);
        return;
    }
    let body = texts::line(&value);
    drop(value);
    let headers = {
        let upstream = lock(&conn.upstream);
        McpRequestHeaders {
            protocol_version: (call.method.as_deref() != Some("initialize"))
                .then(|| upstream.protocol_version.clone())
                .flatten(),
            session_id: upstream.session_id.clone(),
        }
    };
    let before = hub.clock.now().mono.saturating_sub(started);
    let reply = hub.connector.mcp_post(&body, &headers);
    let received = hub.clock.now().mono;
    if uncharged(&reply) {
        refund(&conn.session, &call);
    }
    let lines = match reply {
        Ok(reply) => reply::lines(conn, &call, reply),
        Err(err) => failed(&conn.session, &call, &err).into_iter().collect(),
    };
    for line in &lines {
        answer(conn, line);
    }
    let after = hub.clock.now().mono.saturating_sub(received);
    hub.events.overhead(before.saturating_add(after));
}

/// Writes one line; a write that fails means the agent is gone, and its reader ends.
fn answer(conn: &Conn, line: &[u8]) {
    if conn.writer.line(line).is_err() {
        conn.writer.hang_up();
    }
}

/// Steps 1 to 3. `Err(Some(line))` answers and stops; `Err(None)` stops silently.
fn vet(conn: &Conn, value: &mut Value) -> Result<Call, Option<Vec<u8>>> {
    let Value::Object(map) = value else {
        let text = if value.is_array() {
            texts::NO_BATCHES
        } else {
            texts::INVALID_REQUEST
        };
        return Err(Some(texts::rpc_error(&Value::Null, -32600, text)));
    };
    if map.contains_key("clauseo_relay") {
        // One hello per connection: neither forwarded nor logged, and the connection closes.
        conn.session.log_refusal("second_hello");
        conn.writer.hang_up();
        return Err(None);
    }
    let method = match map.get("method") {
        None => None,
        Some(Value::String(method)) => Some(method.clone()),
        Some(_) => return Err(invalid(map)),
    };
    let id = method.as_ref().and_then(|_| map.get("id").cloned());
    let mut call = Call {
        id,
        method,
        spend: None,
        taken: None,
    };
    match call.method.as_deref() {
        Some("initialize" | "server/discover") => blank_capabilities(map, &call)?,
        Some("tools/call") => vet_tools_call(conn, map, &mut call)?,
        Some(method) if looks_like_tools_call(method) => {
            // Not the spend path's method, yet a connector that folds case or trims could read
            // it as one: refused, never forwarded uncounted.
            conn.session.log_refusal("tools_call_variant");
            return Err(invalid(map));
        }
        _ => {}
    }
    Ok(call)
}

/// Whether `method` reads as `tools/call` once trimmed of whitespace and control characters,
/// NFKC-normalised (full-width and other compatibility forms) and lowercased. The exact
/// method is handled before this is asked.
fn looks_like_tools_call(method: &str) -> bool {
    use unicode_normalization::UnicodeNormalization as _;
    let padding = |c: char| c.is_whitespace() || c.is_control();
    let trimmed = method.trim_matches(padding);
    // Ten characters of at most 4 bytes each; anything longer is another method, at no cost.
    if trimmed.len() > 40 {
        return false;
    }
    let folded: String = trimmed.nfkc().collect::<String>().to_lowercase();
    folded.trim_matches(padding) == "tools/call"
}

/// `Invalid Request` for a request, nothing for a notification.
fn invalid(map: &Map<String, Value>) -> Option<Vec<u8>> {
    map.contains_key("id")
        .then(|| texts::rpc_error(&Value::Null, -32600, texts::INVALID_REQUEST))
}

/// Step 2: `params.capabilities` becomes `{}`; params that are not an object are refused.
fn blank_capabilities(map: &mut Map<String, Value>, call: &Call) -> Result<(), Option<Vec<u8>>> {
    match map.entry("params").or_insert_with(|| json!({})) {
        Value::Object(params) => {
            params.insert("capabilities".to_owned(), json!({}));
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            Err(call
                .id
                .as_ref()
                .map(|id| texts::rpc_error(id, -32602, "Invalid params")))
        }
    }
}

/// Step 3: the id, the tool name, the caps and the credential; sets `call.spend`.
fn vet_tools_call(
    conn: &Conn,
    map: &Map<String, Value>,
    call: &mut Call,
) -> Result<(), Option<Vec<u8>>> {
    let session = &conn.session;
    let Some(id) = call
        .id
        .as_ref()
        .filter(|id| id.is_string() || id.is_number())
    else {
        session.log_refusal("tools_call_without_id");
        return Err(invalid(map));
    };
    let Some(name) = map
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
    else {
        session.log_refusal("tools_call_without_name");
        return Err(Some(texts::rpc_error(id, -32602, texts::BAD_TOOL_NAME)));
    };
    let spend = Spend::of(name);
    call.spend = Some(spend);
    // Only a call that can go out is counted: a credential first, then an open session.
    if let Err(err) = session.hub.connector.bearer() {
        return Err(failed(session, call, &Error::Auth(err)));
    }
    if session.phase() != Phase::Open {
        return Err(None);
    }
    match take(session, spend) {
        Ok(taken) => call.taken = Some(taken),
        Err(cap) => {
            let (reason, text) = match cap {
                Cap::Turn => ("turn_cap", texts::TURN_CAP),
                Cap::Day => ("day_cap", texts::DAY_CAP),
                Cap::Recharge => ("recharge_cap", texts::RECHARGE_CAP),
            };
            session.log_refusal(reason);
            return Err(Some(texts::tool_error(id, text)));
        }
    }
    Ok(())
}

/// Counts one call of `spend` against the session's turn and, when paid, the install's day. The
/// day is read under the day's lock, so two calls at midnight count in clock order. A paid call
/// returns once `day.json` holds its count (or a newer one), written outside the locks.
fn take(session: &Session, spend: Spend) -> Result<Taken, Cap> {
    let hub = &session.hub;
    let (taken, seq) = {
        let mut caps = lock(&session.caps);
        let mut day = lock(&hub.day);
        let today = match spend {
            Spend::Paid => local_date(hub.clock.now().wall.as_millis(), &*hub.local_day),
            Spend::Free | Spend::Recharge => String::new(),
        };
        let taken = caps.take_call(spend, &mut day, &today)?;
        (taken, day.seq())
    };
    if spend == Spend::Paid {
        hub.day_writer.persist(&hub.day, seq);
    }
    Ok(taken)
}

/// Gives back what `call` counted, and writes the day's count if it changed.
fn refund(session: &Session, call: &Call) {
    if let Some(taken) = &call.taken {
        let seq = {
            let mut caps = lock(&session.caps);
            let mut day = lock(&session.hub.day);
            caps.refund(taken, &mut day);
            day.seq()
        };
        session.hub.day_writer.persist(&session.hub.day, seq);
    }
}

/// Whether the connector surely did not charge the call: it refused the credential (a 401,
/// after S28's refresh and retry). A network failure or a timeout may have reached it after the
/// charge, so it is not refunded.
fn uncharged(reply: &Result<McpReply, Error>) -> bool {
    match reply {
        Ok(reply) => reply.status == 401,
        Err(err) => matches!(err, Error::Auth(_)),
    }
}

/// The answer to a call that failed before or at the connector, and the event it means.
fn failed(session: &Session, call: &Call, err: &Error) -> Option<Vec<u8>> {
    let ended = match err {
        Error::Auth(AuthError::NotAdded) => Some(CredentialEnd::NotAdded),
        Error::Auth(AuthError::SignedOut) => Some(CredentialEnd::SignedOut),
        Error::Auth(AuthError::KeyRevoked) => Some(CredentialEnd::KeyRevoked),
        Error::Auth(AuthError::Store(_) | AuthError::Network(_))
        | Error::Http { .. }
        | Error::Network(_)
        | Error::Parse(_)
        | Error::InsufficientCredits(_)
        | Error::NotOurPayLink
        | Error::BadKeyFormat
        | Error::RechargeTimedOut
        | Error::Cancelled
        | Error::Store(_)
        | Error::BadOrigin => None,
    };
    if let Some(ended) = ended {
        session.log_refusal("no_credential");
        session.hub.events.event(RelayEvent::CredentialEnded {
            session: session.key,
            ended,
        });
    } else {
        tracing::info!(session = %session.key.id(), method = call.log_method(), error = %describe(err), "case-law relay: the call failed");
    }
    let text = match err {
        Error::Auth(AuthError::KeyRevoked) => texts::KEY_REVOKED.to_owned(),
        Error::Auth(AuthError::NotAdded | AuthError::SignedOut | AuthError::Store(_)) => {
            texts::SIGNED_OUT.to_owned()
        }
        Error::Http { status } => texts::http_status(*status),
        // S28's own body limit (8 MB): too large either way.
        Error::Network(kind) if kind == "answer too large" => texts::REPLY_TOO_LARGE.to_owned(),
        Error::Auth(AuthError::Network(_))
        | Error::Network(_)
        | Error::Parse(_)
        | Error::InsufficientCredits(_)
        | Error::NotOurPayLink
        | Error::BadKeyFormat
        | Error::RechargeTimedOut
        | Error::Cancelled
        | Error::Store(_)
        | Error::BadOrigin => texts::NO_ANSWER.to_owned(),
    };
    call.id
        .as_ref()
        .map(|id| texts::failure(id, call.tools_call(), &text))
}

/// An error by kind, for the log: never a URL or a body.
fn describe(err: &Error) -> String {
    match err {
        Error::Http { status } => format!("HTTP {status}"),
        Error::Network(kind) => format!("network: {kind}"),
        Error::Parse(_) => "the answer did not parse".to_owned(),
        Error::Auth(AuthError::Network(_)) => "the refresh did not answer".to_owned(),
        Error::Auth(AuthError::Store(_)) | Error::Store(_) => "the keychain failed".to_owned(),
        Error::Auth(AuthError::NotAdded | AuthError::SignedOut | AuthError::KeyRevoked) => {
            "no credential".to_owned()
        }
        Error::InsufficientCredits(_)
        | Error::NotOurPayLink
        | Error::BadKeyFormat
        | Error::RechargeTimedOut
        | Error::Cancelled
        | Error::BadOrigin => "other".to_owned(),
    }
}

/// A method name for the log: up to 64 letters, digits, `/`, `_`, `-` or `.`; else `other`.
fn log_name(method: Option<&str>) -> &str {
    match method {
        None => "response",
        Some(method)
            if (1..=64).contains(&method.len())
                && method.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.')
                }) =>
        {
            method
        }
        Some(_) => "other",
    }
}

#[cfg(test)]
mod tests;
