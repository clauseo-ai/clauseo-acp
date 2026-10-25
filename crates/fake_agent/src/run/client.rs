//! Lines from the client: classification, and matching them against `expect` steps.

use serde_json::Value;

use crate::pattern::{check_off, cut, matches, short, substitute, uses_cwd};
use crate::scenario::{Expect, ExpectResponse, Reply, Wait};

/// One parsed client line.
#[derive(Debug)]
pub(super) enum Incoming {
    Request {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
    Response {
        id: Value,
        reply: Reply,
    },
}

/// Parses a client line. Anything that is not a JSON-RPC 2.0 request, notification or response
/// is malformed, and a malformed line is a mismatch.
pub(super) fn classify(line: &str) -> Result<Incoming, String> {
    let malformed = || format!("malformed line from the client: {}", cut(line));
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(line) else {
        return Err(malformed());
    };
    if map.get("jsonrpc") != Some(&Value::String("2.0".to_owned())) {
        return Err(malformed());
    }
    let params = map.remove("params");
    let raw_id = map.remove("id");
    let has_id = raw_id.is_some();
    let id = raw_id.filter(|id| id.is_string() || id.is_number());
    match (
        map.remove("method"),
        id,
        map.remove("result"),
        map.remove("error"),
    ) {
        (Some(Value::String(method)), Some(id), None, None) => {
            Ok(Incoming::Request { id, method, params })
        }
        (Some(Value::String(method)), None, None, None) if !has_id => {
            Ok(Incoming::Notification { method, params })
        }
        (None, Some(id), Some(result), None) => Ok(Incoming::Response {
            id,
            reply: Reply::Result(result),
        }),
        (None, Some(id), None, Some(error)) => Ok(Incoming::Response {
            id,
            reply: Reply::Error(error),
        }),
        _ => Err(malformed()),
    }
}

impl Incoming {
    /// How a message names this line: `session/new` for a request, `notification session/cancel`,
    /// `response to perm-1`.
    pub(super) fn describe(&self) -> String {
        match self {
            Incoming::Request { method, .. } => method.clone(),
            Incoming::Notification { method, .. } => format!("notification {method}"),
            Incoming::Response { id, .. } => format!("response to {}", id_text(id)),
        }
    }
}

fn id_text(id: &Value) -> String {
    match id {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// How a message names what `wait` expects.
pub(super) fn describe_wait(wait: &Wait) -> String {
    match wait {
        Wait::Expect(expect) => describe_expect(expect),
        Wait::Response(response) => format!("response to {}", response.id),
    }
}

fn describe_expect(expect: &Expect) -> String {
    let kind = if expect.label.is_some() {
        "request"
    } else {
        "notification"
    };
    format!("{kind} {}", expect.method)
}

/// Why `incoming` does not match `wait`. `Identity` means it is another message altogether;
/// `Content` means it is the right message with the wrong params or reply.
pub(super) enum Miss {
    Identity(String),
    Content(String),
}

impl Miss {
    pub(super) fn into_message(self) -> String {
        match self {
            Miss::Identity(message) | Miss::Content(message) => message,
        }
    }
}

/// Matches `incoming` against `wait`, with `{{cwd}}` in patterns replaced by `cwd`.
pub(super) fn match_wait(wait: &Wait, incoming: &Incoming, cwd: Option<&str>) -> Result<(), Miss> {
    match wait {
        Wait::Expect(expect) => match_expect(expect, incoming, cwd),
        Wait::Response(response) => match_response(response, incoming, cwd),
    }
}

fn match_expect(expect: &Expect, incoming: &Incoming, cwd: Option<&str>) -> Result<(), Miss> {
    let params = match (incoming, expect.label.is_some()) {
        (Incoming::Request { method, params, .. }, true)
        | (Incoming::Notification { method, params }, false)
            if *method == expect.method =>
        {
            params.as_ref()
        }
        _ => {
            return Err(Miss::Identity(format!(
                "expected {}, got {}",
                describe_expect(expect),
                incoming.describe()
            )));
        }
    };
    let method = &expect.method;
    if let Some(pattern) = &expect.params {
        let pattern = placed(pattern, cwd).map_err(Miss::Content)?;
        matches(&pattern, params, "")
            .map_err(|m| Miss::Content(format!("{method} params: {m}")))?;
    }
    check_off(params, &expect.off).map_err(|m| Miss::Content(format!("{method} params: {m}")))
}

fn match_response(
    response: &ExpectResponse,
    incoming: &Incoming,
    cwd: Option<&str>,
) -> Result<(), Miss> {
    let wanted = Value::String(response.id.clone());
    let Incoming::Response { id, reply } = incoming else {
        return Err(Miss::Identity(format!(
            "expected response to {}, got {}",
            response.id,
            incoming.describe()
        )));
    };
    if *id != wanted {
        return Err(Miss::Identity(format!(
            "expected response to {}, got {}",
            response.id,
            incoming.describe()
        )));
    }
    let (pattern, actual, field) = match (&response.reply, reply) {
        (Reply::Result(pattern), Reply::Result(actual)) => (pattern, actual, "result"),
        (Reply::Error(pattern), Reply::Error(actual)) => (pattern, actual, "error"),
        (Reply::Result(_), Reply::Error(actual)) => {
            return Err(Miss::Content(format!(
                "expected a result in the response to {}, got error {}",
                response.id,
                short(actual)
            )));
        }
        (Reply::Error(_), Reply::Result(actual)) => {
            return Err(Miss::Content(format!(
                "expected an error in the response to {}, got result {}",
                response.id,
                short(actual)
            )));
        }
    };
    let pattern = placed(pattern, cwd).map_err(Miss::Content)?;
    matches(&pattern, Some(actual), "")
        .map_err(|m| Miss::Content(format!("response to {} {field}: {m}", response.id)))
}

/// `value` with `{{cwd}}` replaced, or an error when it needs a cwd and there is none yet.
pub(super) fn placed(value: &Value, cwd: Option<&str>) -> Result<Value, String> {
    match cwd {
        Some(cwd) => Ok(substitute(value, cwd)),
        None if uses_cwd(value) => Err(NO_CWD.to_owned()),
        None => Ok(value.clone()),
    }
}

/// The message when `{{cwd}}` or a file step runs before any session.
pub(super) const NO_CWD: &str =
    "no current cwd: `{{cwd}}` and file steps need a session/new or session/load request first";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_kinds_and_malformed() {
        let request = classify(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#);
        assert!(matches!(request, Ok(Incoming::Request { .. })));
        let note = classify(r#"{"jsonrpc":"2.0","method":"session/cancel","params":{}}"#);
        assert!(matches!(note, Ok(Incoming::Notification { .. })));
        let response = classify(r#"{"jsonrpc":"2.0","id":"perm-1","result":{}}"#);
        assert!(matches!(response, Ok(Incoming::Response { .. })));
        for bad in [
            "",
            "not json",
            "[1]",
            r#"{"id":0,"method":"initialize"}"#,
            r#"{"jsonrpc":"1.0","id":0,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":"x","result":{},"error":{}}"#,
            r#"{"jsonrpc":"2.0","method":"x","result":{}}"#,
            r#"{"jsonrpc":"2.0"}"#,
        ] {
            assert!(classify(bad).is_err(), "{bad}");
        }
    }
}
