//! Step 5, the reply, read without building it as a tree. S28 hands back the connector's JSON
//! raw (up to 8 MB, 16 in flight per session); a parsed `Value` of a dense array costs about 32
//! bytes per element, so only what the relay needs is read, through borrowed structs:
//!
//! - each message's top-level `id`, `method`, `result` and `error`, as raw slices (a member
//!   that is present counts, even as `null`); the rest is skipped unparsed;
//! - the `id`, parsed only when it is small ([`MAX_ID_BYTES`]; a larger one answers nothing);
//! - `initialize`'s `result.protocolVersion`;
//! - a `tools/call`'s `result.isError` and, only when it is `true`, its `structuredContent` up to
//!   [`MAX_STRUCTURED_BYTES`] (a larger one is `OtherError`, logged), which
//!   [`classify_tool_result_for`] reads from a small value rebuilt around it.
//!
//! The answer's own bytes are the line, with any raw `\n` or `\r` (in valid JSON only whitespace
//! between tokens) made a space so it stays one line. A message with a duplicate top-level key
//! does not read, so it answers nothing: the agent gets the no-answer error instead, which is
//! conservative.

use clauseo_connectors::{McpBody, McpReply, ToolOutcome, classify_tool_result_for};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::super::super::limits::{MAX_REPLY_BYTES, Spend};
use super::super::super::{RelayEvent, texts};
use super::super::conn::Conn;
use super::super::{Session, lock};
use super::{Call, log_name};

/// The largest raw `id` compared with the request's; a larger one answers nothing.
pub(super) const MAX_ID_BYTES: usize = 1024;
/// The largest `structuredContent` of an error result that is read; a larger one is
/// `OtherError`.
pub(super) const MAX_STRUCTURED_BYTES: usize = 64 * 1024;
/// The largest raw `method` or `protocolVersion` read as a string; a larger one is unusable.
const MAX_NAME_BYTES: usize = 256;

/// A JSON-RPC message's top level, borrowed from its raw text.
#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow, default, deserialize_with = "present")]
    id: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    method: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    result: Option<&'a RawValue>,
    #[serde(borrow, default, deserialize_with = "present")]
    error: Option<&'a RawValue>,
}

/// `initialize`'s result: its `protocolVersion` only.
#[derive(Deserialize)]
struct InitializeResult<'a> {
    #[serde(
        rename = "protocolVersion",
        borrow,
        default,
        deserialize_with = "present"
    )]
    protocol_version: Option<&'a RawValue>,
}

/// A `tools/call` result: what says it is an error, and why.
#[derive(Deserialize)]
struct ToolResult<'a> {
    #[serde(rename = "isError", borrow, default, deserialize_with = "present")]
    is_error: Option<&'a RawValue>,
    #[serde(
        rename = "structuredContent",
        borrow,
        default,
        deserialize_with = "present"
    )]
    structured: Option<&'a RawValue>,
}

/// A member that is present, whatever its value (`null` included); a missing one is the
/// field's default, `None`. Plain `Option` would read `null` as missing.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(deserializer).map(Some)
}

/// `raw` read as the object `T`; `None` when it is not an object or does not read (a
/// duplicate key among `T`'s fields, say). The object check comes first: serde's derived
/// structs would also read an array, by position.
fn object<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Option<T> {
    if !raw.get().starts_with('{') {
        return None;
    }
    serde_json::from_str(raw.get()).ok()
}

/// A small raw string (at most [`MAX_NAME_BYTES`]) as text; `None` for anything else.
fn small_string(raw: &RawValue) -> Option<String> {
    if raw.get().len() > MAX_NAME_BYTES {
        return None;
    }
    serde_json::from_str(raw.get()).ok()
}

/// Step 5 for a reply that came back.
pub(super) fn lines(conn: &Conn, call: &Call, reply: McpReply) -> Vec<Vec<u8>> {
    let session = &conn.session;
    if let Some(sid) = reply.session_id.as_deref() {
        match checked_session_id(sid) {
            Some(sid) => lock(&conn.upstream).session_id = Some(sid.to_owned()),
            None => {
                tracing::warn!(session = %session.key.id(), "case-law relay: an Mcp-Session-Id was not usable");
            }
        }
    }
    let Some(id) = call.id.as_ref() else {
        // A notification or the agent's response: nothing goes back.
        return Vec::new();
    };
    if reply.status == 401 && call.tools_call() {
        return vec![texts::tool_error(id, texts::SIGNED_OUT)];
    }
    if reply.status >= 400 {
        return vec![texts::failure(
            id,
            call.tools_call(),
            &texts::http_status(reply.status),
        )];
    }
    let raws = match reply.body {
        McpBody::Json(raw) => vec![raw],
        McpBody::Events(raws) => raws,
        McpBody::Empty => Vec::new(),
    };
    let Some((raw, envelope)) = answer_to(session, id, &raws) else {
        return vec![texts::failure(id, call.tools_call(), texts::NO_ANSWER)];
    };
    if let Some(result) = envelope.result {
        read_result(conn, call, result);
    }
    if raw.get().len() > MAX_REPLY_BYTES {
        tracing::info!(session = %session.key.id(), method = call.log_method(), "case-law relay: the reply was too large");
        return vec![texts::failure(
            id,
            call.tools_call(),
            texts::REPLY_TOO_LARGE,
        )];
    }
    vec![one_line(raw)]
}

/// The answer's raw bytes as one line: a raw `\n` or `\r` (whitespace between tokens, the only
/// place valid JSON has them) becomes a space.
pub(super) fn one_line(raw: &RawValue) -> Vec<u8> {
    raw.get()
        .bytes()
        .map(|byte| match byte {
            b'\n' | b'\r' => b' ',
            other => other,
        })
        .collect()
}

/// The first value that answers request `id`: a response (`result` or `error`, no `method`)
/// with that very `id`. Everything else is dropped and logged by kind: a request from the
/// connector by its method, a notification, an answer to another id, or a value that does not
/// read as a message. The agent gets only what the relay vetted, and exactly one answer.
fn answer_to<'a>(
    session: &Session,
    id: &Value,
    raws: &'a [Box<RawValue>],
) -> Option<(&'a RawValue, Envelope<'a>)> {
    let mut answer = None;
    for raw in raws {
        let envelope = object::<Envelope<'_>>(raw);
        let is_answer = envelope.as_ref().is_some_and(|message| {
            message.method.is_none()
                && (message.result.is_some() || message.error.is_some())
                && message.id.is_some_and(|theirs| same_id(theirs, id))
        });
        match envelope {
            Some(message) if is_answer && answer.is_none() => {
                answer = Some((&**raw, message));
            }
            message => {
                let method = message.as_ref().and_then(|message| message.method);
                let name = method.and_then(small_string);
                let kind = match message.map(|message| (method, message.id)) {
                    None => "unreadable",
                    Some((Some(_), Some(_))) => log_name(name.as_deref()),
                    Some((Some(_), None)) => "notification",
                    Some((None, _)) => "foreign_answer",
                };
                tracing::warn!(session = %session.key.id(), kind, "case-law relay: dropped a message from the connector");
            }
        }
    }
    answer
}

/// Whether the raw `theirs` is the request's `id`: parsed, and only when it is small.
pub(super) fn same_id(theirs: &RawValue, id: &Value) -> bool {
    theirs.get().len() <= MAX_ID_BYTES
        && serde_json::from_str::<Value>(theirs.get()).is_ok_and(|theirs| theirs == *id)
}

/// Reads a response's raw `result`: `initialize`'s protocol version, and a `tools/call`
/// result's meaning.
fn read_result(conn: &Conn, call: &Call, result: &RawValue) {
    let session = &conn.session;
    if call.method.as_deref() == Some("initialize") {
        let version = object::<InitializeResult<'_>>(result)
            .and_then(|result| result.protocol_version)
            .and_then(small_string);
        match version.as_deref().and_then(checked_protocol_version) {
            Some(version) => lock(&conn.upstream).protocol_version = Some(version.to_owned()),
            None => {
                tracing::warn!(session = %session.key.id(), "case-law relay: initialize gave no usable protocolVersion");
            }
        }
    }
    let Some(spend) = call.spend else {
        return;
    };
    let hub = &session.hub;
    match tool_outcome(session, result) {
        ToolOutcome::OutOfCredit(payment) => hub.events.event(RelayEvent::OutOfCredit {
            session: session.key,
            payment,
        }),
        ToolOutcome::Ok if spend == Spend::Paid => hub.events.event(RelayEvent::PaidResult {
            session: session.key,
        }),
        ToolOutcome::Ok | ToolOutcome::OtherError => {}
    }
}

/// What a raw `tools/call` result means. A result that is not an object is not an error, as
/// before; one that is an object but does not read (a duplicate key) is `OtherError`.
fn tool_outcome(session: &Session, result: &RawValue) -> ToolOutcome {
    if !result.get().starts_with('{') {
        return ToolOutcome::Ok;
    }
    let Some(read) = object::<ToolResult<'_>>(result) else {
        tracing::info!(session = %session.key.id(), "case-law relay: a tool result did not read");
        return ToolOutcome::OtherError;
    };
    let is_error = read
        .is_error
        .is_some_and(|raw| serde_json::from_str::<bool>(raw.get()).is_ok_and(|yes| yes));
    if !is_error {
        return ToolOutcome::Ok;
    }
    let Some(structured) = read.structured else {
        return ToolOutcome::OtherError;
    };
    if structured.get().len() > MAX_STRUCTURED_BYTES {
        tracing::info!(session = %session.key.id(), "case-law relay: an error result's structuredContent was too large to read");
        return ToolOutcome::OtherError;
    }
    let Ok(structured) = serde_json::from_str::<Value>(structured.get()) else {
        return ToolOutcome::OtherError;
    };
    let small = json!({"isError": true, "structuredContent": structured});
    let hub = &session.hub;
    classify_tool_result_for(hub.connector.known(), &hub.origin, &small)
}

/// A `protocolVersion` fit for a header: 1 to 32 ASCII letters, digits, `-`, `.` or `_`.
pub(super) fn checked_protocol_version(version: &str) -> Option<&str> {
    let fits = (1..=32).contains(&version.len())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'));
    fits.then_some(version)
}

/// An `Mcp-Session-Id` fit for a header: 1 to 256 visible ASCII characters (MCP's own rule).
pub(super) fn checked_session_id(sid: &str) -> Option<&str> {
    let fits =
        (1..=256).contains(&sid.len()) && sid.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
    fits.then_some(sid)
}
