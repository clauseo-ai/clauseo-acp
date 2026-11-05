//! The relay's fixed English texts and the JSON-RPC lines that carry them. They are for the
//! agent, not interface words, so they are not on the copy page. A `tools/call` gets an
//! `isError` tool result, which the model reads; any other request a JSON-RPC error.

use serde_json::{Value, json};

/// The handshake's error codes, as `clauseo-mcp connector` reads them.
pub(crate) const BAD_TOKEN: &str = "bad_token";
pub(crate) const WRONG_CONNECTOR: &str = "wrong_connector";
pub(crate) const UNSUPPORTED_VERSION: &str = "unsupported_version";
pub(crate) const BUSY: &str = "busy";
pub(crate) const CLOSED: &str = "closed";

pub(crate) const LINE_TOO_LONG: &str = "Line too long for Clauseo's case-law relay.";
pub(crate) const NO_BATCHES: &str = "Clauseo's case-law relay does not accept JSON-RPC batches.";
pub(crate) const PARSE_ERROR: &str = "Parse error";
pub(crate) const INVALID_REQUEST: &str = "Invalid Request";
pub(crate) const BAD_TOOL_NAME: &str =
    "Invalid params: Clauseo's case-law relay needs params.name to be a string.";
pub(crate) const TURN_CAP: &str = "This turn has reached Clauseo's limit of 60 case-law calls. \
     Answer with what you have, or ask the lawyer to continue in a new message.";
pub(crate) const DAY_CAP: &str = "Clauseo's daily limit of 600 case-law calls on this computer is \
     reached. It resets at midnight. Tell the lawyer.";
pub(crate) const RECHARGE_CAP: &str = "This turn has reached Clauseo's limit of 5 recharge calls. \
     Tell the lawyer to add credits under Settings, Connectors.";
pub(crate) const REPLY_TOO_LARGE: &str = "The case-law reply was over Clauseo's 4 MB limit. Ask \
     for less, for example fewer documents or a smaller part.";
pub(crate) const SIGNED_OUT: &str =
    "Case law is signed out. Tell the lawyer to sign in again under Settings, Connectors.";
pub(crate) const KEY_REVOKED: &str = "The firm's case-law API key no longer works. Tell the \
     lawyer to add a new key under Settings, Connectors.";
pub(crate) const NO_ANSWER: &str = "The case-law service did not answer. Try again later.";

/// "The case-law service answered HTTP <status>. Try again later."
pub(crate) fn http_status(status: u16) -> String {
    format!("The case-law service answered HTTP {status}. Try again later.")
}

/// `{"clauseo_relay":1,"ok":true}`.
pub(crate) fn hello_ok() -> Vec<u8> {
    br#"{"clauseo_relay":1,"ok":true}"#.to_vec()
}

/// `{"clauseo_relay":1,"ok":false,"error":"<code>"}`.
pub(crate) fn hello_refused(code: &str) -> Vec<u8> {
    line(&json!({"clauseo_relay": 1, "ok": false, "error": code}))
}

/// A JSON-RPC error line.
pub(crate) fn rpc_error(id: &Value, code: i64, message: &str) -> Vec<u8> {
    line(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
}

/// An `isError` tool result line.
pub(crate) fn tool_error(id: &Value, text: &str) -> Vec<u8> {
    line(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": text}], "isError": true},
    }))
}

/// `text` for the agent: a tool result for a `tools/call`, else JSON-RPC error `-32603`.
pub(crate) fn failure(id: &Value, tools_call: bool, text: &str) -> Vec<u8> {
    if tools_call {
        tool_error(id, text)
    } else {
        rpc_error(id, -32603, text)
    }
}

/// `value` as compact JSON. A `Value` always serialises; the fallback is never sent.
pub(crate) fn line(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}
