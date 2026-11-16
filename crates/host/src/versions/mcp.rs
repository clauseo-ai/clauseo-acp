//! One JSON-RPC message from the agent and its answer. `initialize` names the server and offers
//! tools only; `ping` is answered empty; `tools/list` gives the two tools; `tools/call` runs one
//! after the per-turn cap; any other method is `Method not found`. A notification, or a message
//! with no method (the agent's own response), gets nothing back. A tool's refusal is a result
//! with `isError` and one sentence, so the agent reads it; an unknown tool is the JSON-RPC
//! error MCP gives it.

use serde_json::{Map, Value, json};

use super::texts::{self, Refusal};
use super::{Backend, MAX_LIMIT, SERVER_NAME, list, lock, read};

/// The protocol versions this server speaks: it answers with the agent's when it is one of them,
/// else with [`LATEST`].
const PROTOCOL_VERSIONS: [&str; 3] = [LATEST, "2025-03-26", "2024-11-05"];

/// The newest protocol version it speaks.
const LATEST: &str = "2025-06-18";

/// What one tool call gives: the result, or a refusal sentence.
pub(super) type Outcome = Result<Value, Refusal>;

/// The answer to `message`, or `None` when none is due.
pub(super) fn answer(backend: &Backend, message: &Value) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(error(&Value::Null, -32600, "Invalid Request"));
    };
    let method = object.get("method")?;
    let id = object.get("id")?;
    if !(id.is_string() || id.is_number()) {
        return Some(error(&Value::Null, -32600, "Invalid Request"));
    }
    let Some(method) = method.as_str() else {
        return Some(error(id, -32600, "Invalid Request"));
    };
    let params = object.get("params").and_then(Value::as_object);
    let result = match method {
        "initialize" => initialize(params),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => return Some(call(backend, id, params)),
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn initialize(params: Option<&Map<String, Value>>) -> Value {
    let asked = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    let version = PROTOCOL_VERSIONS
        .into_iter()
        .find(|known| Some(*known) == asked)
        .unwrap_or(LATEST);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
    })
}

/// The two tools, as `tools/list` gives them.
pub(super) fn tools() -> Value {
    let annotations = |title: &str| {
        json!({
            "title": title,
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        })
    };
    json!([
        {
            "name": list::NAME,
            "description": texts::LIST_DESCRIPTION,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "before": {
                        "type": "string",
                        "description": "Only versions older than this version id \
                                        (next_before of the page before)."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_LIMIT,
                        "description": "How many versions, 20 if not given."
                    }
                },
                "additionalProperties": false
            },
            "annotations": annotations("List versions"),
        },
        {
            "name": read::NAME,
            "description": texts::READ_DESCRIPTION,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "version": {
                        "type": "string",
                        "description": "A version id from list_versions."
                    },
                    "path": {
                        "type": "string",
                        "description": "The file's path in that version, relative to the \
                                        matter's folder, as list_versions shows it."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Where the page starts, in bytes of text: 0 if not \
                                        given, else next_offset of the page before."
                    }
                },
                "required": ["version", "path"],
                "additionalProperties": false
            },
            "annotations": annotations("Read an earlier version"),
        }
    ])
}

/// A `tools/call`: the cap, then the tool. Arguments that are not an object read as none.
fn call(backend: &Backend, id: &Value, params: Option<&Map<String, Value>>) -> Value {
    let name = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str);
    let empty = Map::new();
    let args = params
        .and_then(|params| params.get("arguments"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let tool: fn(&Backend, &Map<String, Value>) -> Outcome = match name {
        Some(list::NAME) => list::run,
        Some(read::NAME) => read::run,
        Some(_) | None => return error(id, -32602, "Unknown tool"),
    };
    let session = backend.config.session;
    let outcome = if lock(&backend.caps).take_call() {
        tool(backend, args)
    } else {
        Err(Refusal::TooManyCalls)
    };
    let result = match outcome {
        Ok(structured) => {
            let text = summary(&structured);
            json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": structured,
                "isError": false,
            })
        }
        Err(refusal) => {
            tracing::info!(
                matter = %session.matter,
                thread = %session.thread,
                tool = name.unwrap_or_default(),
                refusal = refusal.name(),
                "a version-history call was refused"
            );
            json!({
                "content": [{ "type": "text", "text": refusal.text() }],
                "isError": true,
            })
        }
    };
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The one line `content` carries beside `structuredContent`, which holds the answer itself:
/// Claude Code shows its model only `structuredContent` when a result has one (as the case-law
/// relay found, `connectors/relay/forward/reply.rs`), so the page goes there once and the line
/// only says where it is.
fn summary(structured: &Value) -> String {
    if let Some(versions) = structured.get("versions").and_then(Value::as_array) {
        let more = if structured.get("next_before").is_some() {
            "; older ones page with next_before"
        } else {
            ""
        };
        return format!(
            "{} versions of this matter, newest first, are in structuredContent.versions{more}.",
            versions.len()
        );
    }
    let field = |name: &str| {
        structured
            .get(name)
            .map(ToString::to_string)
            .unwrap_or_default()
    };
    let read = structured
        .get("text")
        .and_then(Value::as_str)
        .map_or(0, str::len);
    format!(
        "{} at version {}: {read} bytes from offset {} of {}, in structuredContent.text.",
        field("path"),
        field("version"),
        field("offset"),
        field("total_bytes"),
    )
}

/// A JSON-RPC error answer.
fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
