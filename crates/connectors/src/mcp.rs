//! What the relay sends and gets back through `mcp_post`, and [`classify_tool_result`], which
//! spots an out-of-credit tool result.

use serde_json::Value;
use serde_json::value::RawValue;

use crate::config::Origin;
use crate::error::Error;
use crate::known::KnownConnector;
use crate::recharge::{PaymentRef, is_insufficient_credits, payment_ref};

/// The case-law server's `initialize` result for protocol version 2025-06-18 as Clauseo ships
/// it (`assets/case_law/initialize.json`): the server's own bytes (connectors main e9448d7),
/// compact, keys sorted, with the production base URL `https://data.clauseo.chat` in its
/// instructions, which the host replaces with the origin's. The relay answers from the last copy
/// the server gave, cached per origin and version; this is the fallback before there is one, and
/// the other versions' answers are derived from it. The fake connector serves it too.
pub const CASE_LAW_INITIALIZE: &str = include_str!("../assets/case_law/initialize.json");
/// The server's `tools/list` result for 2025-06-18 and 2025-11-25
/// (`assets/case_law/tools_list.json`): the fallback, as [`CASE_LAW_INITIALIZE`] is.
pub const CASE_LAW_TOOLS_LIST: &str = include_str!("../assets/case_law/tools_list.json");
/// The server's `tools/list` result for 2025-03-26 (and with no version header): each tool's
/// title in its annotations, no output schema (`assets/case_law/tools_list_2025-03-26.json`).
pub const CASE_LAW_TOOLS_LIST_2025_03_26: &str =
    include_str!("../assets/case_law/tools_list_2025-03-26.json");

/// The MCP headers the relay forwards; each is sent only when given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpRequestHeaders {
    /// `MCP-Protocol-Version`.
    pub protocol_version: Option<String>,
    /// `Mcp-Session-Id`.
    pub session_id: Option<String>,
}

/// The connector's answer to one MCP POST.
#[derive(Debug, Clone, PartialEq)]
pub struct McpReply {
    /// The HTTP status.
    pub status: u16,
    /// `Mcp-Session-Id`, when the connector sent one.
    pub session_id: Option<String>,
    /// The body.
    pub body: McpBody,
}

/// An MCP answer's body, its JSON raw: checked to be JSON, never built as a tree, so a reply of
/// up to 8 MB costs about its own size whatever it holds.
#[derive(Debug, Clone)]
pub enum McpBody {
    /// `application/json`: one JSON-RPC message, as the connector wrote it (trimmed).
    Json(Box<RawValue>),
    /// `text/event-stream`: one value per `data:` event. An event of several `data:` lines
    /// holds them joined with `\n`, which JSON reads as whitespace.
    Events(Vec<Box<RawValue>>),
    /// No body (202 Accepted).
    Empty,
}

/// Equal when the raw texts are the same, byte for byte.
impl PartialEq for McpBody {
    fn eq(&self, other: &McpBody) -> bool {
        match (self, other) {
            (McpBody::Json(left), McpBody::Json(right)) => left.get() == right.get(),
            (McpBody::Events(left), McpBody::Events(right)) => {
                left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.get() == b.get())
            }
            (McpBody::Empty, McpBody::Empty) => true,
            (McpBody::Json(_) | McpBody::Events(_) | McpBody::Empty, _) => false,
        }
    }
}

/// What a `tools/call` result means for the lawyer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    /// Not an error.
    Ok,
    /// Out of credit, with the payment the connector opened.
    OutOfCredit(PaymentRef),
    /// Any other error result.
    OtherError,
}

/// Reads a JSON-RPC `result` of `tools/call` (a whole response's `result` is taken too):
/// `isError: true` with `structuredContent.error.code == "insufficient_credits"` is
/// `OutOfCredit`, with `payment.recharge_id` and `payment.url` through [`PayUrl::parse`];
/// any other `isError: true` is `OtherError`; anything else is `Ok`. Production pay links only;
/// see [`classify_tool_result_for`].
///
/// [`PayUrl::parse`]: crate::PayUrl::parse
pub fn classify_tool_result(connector: &KnownConnector, result: &Value) -> ToolOutcome {
    classify_tool_result_for(connector, &Origin::Production, result)
}

/// [`classify_tool_result`] with pay links read for `origin` ([`PayUrl::parse_for`]).
///
/// [`PayUrl::parse_for`]: crate::PayUrl::parse_for
pub fn classify_tool_result_for(
    connector: &KnownConnector,
    origin: &Origin,
    result: &Value,
) -> ToolOutcome {
    let result = match result.get("result") {
        Some(inner) if result.get("jsonrpc").is_some() => inner,
        _ => result,
    };
    if result.get("isError").and_then(Value::as_bool) != Some(true) {
        return ToolOutcome::Ok;
    }
    match result.get("structuredContent") {
        Some(content) if is_insufficient_credits(content) => {
            ToolOutcome::OutOfCredit(payment_ref(content, connector, origin))
        }
        _ => ToolOutcome::OtherError,
    }
}

/// The values of a `text/event-stream` body: one per event with `data:` lines (joined with
/// `\n`), each checked as JSON and kept raw. Comments, `event:`, `id:` and `retry:` lines are
/// skipped.
pub(crate) fn parse_event_stream(body: &str) -> Result<Vec<Box<RawValue>>, Error> {
    let mut values = Vec::new();
    let mut data: Option<String> = None;
    let mut flush = |data: &mut Option<String>| -> Result<(), Error> {
        if let Some(text) = data.take() {
            let value = crate::calls::raw_json(text)
                .map_err(|err| crate::error::parse_error("event stream", &err))?;
            values.push(value);
        }
        Ok(())
    };
    for line in body.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            flush(&mut data)?;
            continue;
        }
        let (field, rest) = line.split_once(':').unwrap_or((line, ""));
        if field != "data" {
            continue;
        }
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        match &mut data {
            Some(text) => {
                text.push('\n');
                text.push_str(rest);
            }
            None => data = Some(rest.to_owned()),
        }
    }
    flush(&mut data)?;
    Ok(values)
}

#[cfg(test)]
mod tests;
