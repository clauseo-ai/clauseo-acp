//! The WARN lines this crate writes when the agent answers a request with an error, when an
//! answer or a message from the agent does not decode, and when an answer decodes but does not
//! map. They go to the host's log file (decision record 0047: ids, codes and kinds only), so each
//! names the JSON-RPC method and the error's shape, never a value: no message text, no `data`
//! value, no quoted serde value, no path.

use agent_client_protocol::schema::v1 as acp;
use serde_json::Value;

/// Logs an error answer to `method`. A `-32700` whose `data.phase` is `deserialization` is the
/// SDK's own: the agent's answer did not decode into the response type. Anything else is the
/// agent's error, logged by its code, its message's length in bytes and its `data`'s key names.
pub(crate) fn rpc_error(method: &str, error: &acp::Error) {
    let code = i32::from(error.code);
    if let Some(serde) = decode_failure(error) {
        tracing::warn!(
            method,
            error = %serde_category(serde),
            "the agent's response to this request did not decode"
        );
        return;
    }
    tracing::warn!(
        method,
        code,
        message_len = error.message.len(),
        data = %data_shape(error.data.as_ref()),
        "the agent answered this request with an error"
    );
}

/// Logs an answer to `method` that decoded but that the driver cannot use (an id no command
/// could name, a select list of a newer shape).
pub(crate) fn unmapped(method: &str) {
    tracing::warn!(
        method,
        "the agent's response to this request could not be used"
    );
}

/// Logs a message from the agent (`method`, a notification or a request) that did not decode.
pub(crate) fn undecoded(method: &str, serde: &str) {
    tracing::warn!(
        method,
        error = %serde_category(serde),
        "a message from the agent did not decode"
    );
}

/// The serde error text of the SDK's decode failure, when `error` is one: `-32700` with
/// `data.phase == "deserialization"` and `data.error` (agent-client-protocol 3.3.0,
/// `util::json_cast`).
fn decode_failure(error: &acp::Error) -> Option<&str> {
    if error.code != acp::ErrorCode::ParseError {
        return None;
    }
    let data = error.data.as_ref()?;
    if data.get("phase").and_then(Value::as_str) != Some("deserialization") {
        return None;
    }
    data.get("error").and_then(Value::as_str)
}

/// The `data` of an error answer as the log may show it: `none`, the kind of a scalar, or an
/// object's key names in order (each cut at 64 bytes, at most 16 of them).
pub(crate) fn data_shape(data: Option<&Value>) -> String {
    match data {
        None | Some(Value::Null) => "none".to_owned(),
        Some(Value::Bool(_)) => "bool".to_owned(),
        Some(Value::Number(_)) => "number".to_owned(),
        Some(Value::String(_)) => "string".to_owned(),
        Some(Value::Array(items)) => format!("array of {}", items.len()),
        Some(Value::Object(map)) => {
            let keys: Vec<&str> = map.keys().take(16).map(|key| cut(key, 64)).collect();
            let more = if map.len() > 16 { ", …" } else { "" };
            format!("keys [{}{more}]", keys.join(", "))
        }
    }
}

/// A `serde_json` error's text with every value it quotes left out: its category, the field it
/// names when the field is the schema's (`missing field`, `duplicate field`), what the schema
/// expected (the schema's own words), and its line and column when it has them.
pub(crate) fn serde_category(text: &str) -> String {
    let (head, position) = match text.rfind(" at line ") {
        Some(at) => (
            text.get(..at).unwrap_or(text),
            text.get(at + " at ".len()..),
        ),
        None => (text, None),
    };
    let expected = head
        .rfind(", expected ")
        .and_then(|at| head.get(at + 2..))
        .filter(|rest| !rest.contains('"'))
        .map(|rest| cut(rest, 200));
    let category = if let Some(field) = schema_field(head, "missing field `") {
        format!("missing field `{field}`")
    } else if let Some(field) = schema_field(head, "duplicate field `") {
        format!("duplicate field `{field}`")
    } else {
        [
            "unknown variant",
            "unknown field",
            "invalid type",
            "invalid value",
            "invalid length",
            "data did not match any variant",
            "expected",
            "EOF while parsing",
            "trailing characters",
            "recursion limit exceeded",
        ]
        .into_iter()
        .find(|known| head.starts_with(known))
        .unwrap_or("other")
        .to_owned()
    };
    let mut line = category;
    if let Some(expected) = expected {
        line.push_str(", ");
        line.push_str(expected);
    }
    if let Some(position) = position.filter(|position| is_position(position)) {
        line.push(' ');
        line.push_str(position);
    }
    line
}

/// The field after `prefix` up to its closing backtick, when it is a plain identifier (a schema
/// field name, never a value).
fn schema_field<'a>(head: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = head.strip_prefix(prefix)?;
    let field = rest.get(..rest.find('`')?)?;
    let plain = !field.is_empty()
        && field.len() <= 64
        && field
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    plain.then_some(field)
}

/// `line N column M`, digits only.
fn is_position(text: &str) -> bool {
    let mut words = text.split(' ');
    matches!(
        (words.next(), words.next(), words.next(), words.next(), words.next()),
        (Some("line"), Some(line), Some("column"), Some(column), None)
            if !line.is_empty() && !column.is_empty()
                && line.bytes().all(|b| b.is_ascii_digit())
                && column.bytes().all(|b| b.is_ascii_digit())
    )
}

/// `text` cut to at most `max` bytes, on a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;
