//! What the relay sends and gets back through `mcp_post`, and [`classify_tool_result`], which
//! spots an out-of-credit tool result.

use serde_json::Value;

use crate::config::Origin;
use crate::error::Error;
use crate::known::KnownConnector;
use crate::recharge::{PaymentRef, is_insufficient_credits, payment_ref};

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

/// An MCP answer's body.
#[derive(Debug, Clone, PartialEq)]
pub enum McpBody {
    /// `application/json`: one JSON-RPC message.
    Json(Value),
    /// `text/event-stream`: one value per `data:` event.
    Events(Vec<Value>),
    /// No body (202 Accepted).
    Empty,
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
/// `\n`), each parsed as JSON. Comments, `event:`, `id:` and `retry:` lines are skipped.
pub(crate) fn parse_event_stream(body: &str) -> Result<Vec<Value>, Error> {
    let mut values = Vec::new();
    let mut data: Option<String> = None;
    let mut flush = |data: &mut Option<String>| -> Result<(), Error> {
        if let Some(text) = data.take() {
            let value = serde_json::from_str(&text)
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
