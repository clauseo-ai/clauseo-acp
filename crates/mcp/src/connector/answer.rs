//! The lines the connector answers itself, in place of the relay: the "not reachable" error for
//! each request while there is no link, the parse error for a line that is not JSON, and the
//! line-too-long error. Their texts are R24's protocol table, aimed at the agent, and name the
//! connector the mode runs as ([`Subject`]).

use serde_json::{Value, json};

/// What the texts call the connector the mode runs as, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Subject {
    /// What is not available or busy: "Case law".
    what: &'static str,
    /// The relay a line is too long for: "Clauseo's case-law relay".
    relay: &'static str,
}

impl Subject {
    /// The words for connector `name`: `case-law` and `clauseo-versions` by name, any other
    /// name as "This tool" and "Clauseo's relay", which name nothing it was given.
    pub(crate) fn of(name: &str) -> Subject {
        match name {
            "case-law" => Subject {
                what: "Case law",
                relay: "Clauseo's case-law relay",
            },
            "clauseo-versions" => Subject {
                what: "The version history",
                relay: "Clauseo's version-history relay",
            },
            _ => Subject {
                what: "This tool",
                relay: "Clauseo's relay",
            },
        }
    }

    /// The message for a request the relay cannot take.
    fn not_reachable(self) -> String {
        format!(
            "{} is not available in this conversation: Clauseo's relay is not reachable. Tell the lawyer to start a new conversation.",
            self.what
        )
    }

    /// The message for a request past the pending cap: the relay is busy, not gone.
    fn busy(self) -> String {
        format!(
            "{} is busy: too many requests are waiting for Clauseo's relay. Wait for the answers to them, then try again.",
            self.what
        )
    }

    /// The message for a line over the cap.
    fn line_too_long(self) -> String {
        format!("Line too long for {}.", self.relay)
    }
}

/// The replies for one line the agent sent while there is no link: one error per request in it,
/// with the request's `id`. A line that is not JSON gets the parse error, with a null `id`. A
/// notification, or a response, gets nothing.
pub(crate) fn not_reachable(subject: Subject, line: &[u8], reason: &str) -> Vec<String> {
    match serde_json::from_slice::<Value>(line) {
        Err(_) => vec![error(&Value::Null, -32700, "Parse error", None)],
        Ok(Value::Array(items)) => items
            .iter()
            .filter_map(request_id)
            .map(|id| unreachable_for(subject, &id, reason))
            .collect(),
        Ok(value) => request_id(&value)
            .map(|id| unreachable_for(subject, &id, reason))
            .into_iter()
            .collect(),
    }
}

/// The not-reachable error for one request, by its `id`: for a request the link lost or refused.
pub(crate) fn unreachable_for(subject: Subject, id: &Value, reason: &str) -> String {
    error(id, -32000, &subject.not_reachable(), Some(reason))
}

/// The busy error for one request past the pending cap, by its `id`.
pub(crate) fn busy_for(subject: Subject, id: &Value) -> String {
    error(id, -32000, &subject.busy(), Some("busy"))
}

/// The reply for a line over the cap: the line-too-long error, with a null `id`.
pub(crate) fn line_too_long(subject: Subject) -> String {
    error(&Value::Null, -32600, &subject.line_too_long(), None)
}

/// The `id` of a request (an object with `method` and `id`); `None` for anything else.
fn request_id(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if object.contains_key("method") {
        object.get("id").cloned()
    } else {
        None
    }
}

/// A JSON-RPC error reply, one line. `data.reason` is set when a reason is given.
fn error(id: &Value, code: i64, message: &str, reason: Option<&str>) -> String {
    let mut body = json!({ "code": code, "message": message });
    if let Some(reason) = reason {
        body["data"] = json!({ "reason": reason });
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": body }).to_string()
}

#[cfg(test)]
mod tests;
