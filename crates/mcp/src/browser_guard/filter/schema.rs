//! The tools the agent is offered: agent-browser's `tools/list` reply, rewritten so it names
//! only what the guard lets through. The relay calls [`advertised_tools`] only for the reply to
//! a `tools/list` request (`relay/pumps.rs`, `relay/oneshot.rs`), so Codex and Claude alike see
//! the same list, whoever asks.

use serde_json::{Map, Value, json};

use super::{ALLOWED_TOOLS, EXTRA_ARGS_ALLOWED, MAX_TIMEOUT_MS, TIME_KEYS, TOOL_READ, keys};

/// `agent_browser_read` as the guard lets it run: on the open tab only (its `url` is refused,
/// `filter.rs`'s [`super::READ_FETCH_KEYS`]). agent-browser's own text, "Fetch a URL as
/// agent-readable text, preferring text/markdown. Omit url to read the active tab.", offers the
/// `url` the guard refuses, so the tool's description and title are replaced with these.
const READ_DESCRIPTION: &str = "Read the open tab as agent-readable text.";
const READ_TITLE: &str = "Read page";

/// A reply to `tools/list` (`line`, without its newline) as the agent gets it: every tool off
/// [`ALLOWED_TOOLS`] dropped from `result.tools` (`agent_browser_eval` in the core profile), and
/// in each tool kept, its input schema's `properties` cut to the keys [`keys::advertised`]
/// keeps, so no agent is offered `allowedDomains`, `session`, `path` or any other key the guard
/// refuses; `required` loses any key dropped. What stays is told the guard's limits:
/// `extraArgs` lists its allowed tokens as an `enum` (agent-browser's own text names
/// `--allowed-domains` and engines the guard refuses), and each key of [`TIME_KEYS`] gets its
/// `maximum`. `agent_browser_read`'s description and title lose the `url` it no longer takes
/// ([`READ_DESCRIPTION`]). An entry with no string name stays as it is. `None` when `line` is no
/// such reply or nothing changes: it then goes on as read.
pub(crate) fn advertised_tools(line: &[u8]) -> Option<Vec<u8>> {
    let mut reply: Value = serde_json::from_slice(line).ok()?;
    let tools = reply.get_mut("result")?.get_mut("tools")?.as_array_mut()?;
    let before = tools.clone();
    tools.retain(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| ALLOWED_TOOLS.contains(&name))
    });
    for tool in tools.iter_mut() {
        if let Some(name) = tool.get("name").and_then(Value::as_str).map(str::to_owned) {
            advertise(&name, tool);
        }
    }
    if *tools == before {
        return None;
    }
    serde_json::to_vec(&reply).ok()
}

/// Cuts `tool`'s input schema to what passes on tool `name`.
fn advertise(name: &str, tool: &mut Value) {
    if name == TOOL_READ {
        if let Some(description) = tool.get_mut("description") {
            *description = json!(READ_DESCRIPTION);
        }
        if let Some(title) = tool.get_mut("title") {
            *title = json!(READ_TITLE);
        }
    }
    let Some(schema) = tool.get_mut("inputSchema").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.retain(|key, _| keys::advertised(name, key));
        if let Some(extra) = properties.get_mut("extraArgs") {
            *extra = extra_args_schema();
        }
        for key in TIME_KEYS {
            if let Some(time) = properties.get_mut(*key).and_then(Value::as_object_mut) {
                time.insert("maximum".to_owned(), json!(MAX_TIMEOUT_MS));
            }
        }
    }
    let kept: Map<String, Value> = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|key| key.as_str().is_some_and(|key| kept.contains_key(key)));
    }
}

/// `extraArgs` as the guard takes it: an array of [`EXTRA_ARGS_ALLOWED`] tokens.
fn extra_args_schema() -> Value {
    json!({
        "type": "array",
        "items": {"type": "string", "enum": EXTRA_ARGS_ALLOWED},
        "description": "Advanced: extra output flags. Only these are accepted.",
    })
}
