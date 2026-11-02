//! The check on each line the agent sends to agent-browser. It sees only that direction; what
//! agent-browser sends back is relayed untouched (`relay.rs`).
//!
//! A line is parsed once, as a `serde_json::Value`, and only that value moves on: the child gets
//! the guard's own serialisation of it, so a duplicate key, odd whitespace or an escape cannot
//! mean one thing here and another there. agent-browser parses with the same library and keeps
//! the last of duplicate keys too (`cli/src/mcp.rs:661-671` at v0.41.1).
//!
//! The rules, in the order they run (the first that fails names the refusal):
//! 1. Not JSON, or longer than [`MAX_LINE_BYTES`]: `-32700` with id null, nothing forwarded.
//! 2. An array (a batch): `-32600`, nothing forwarded. A scalar: `-32600`, nothing forwarded.
//! 3. An object whose `method` is exactly `tools/call`, with `params.arguments` an object:
//!    a. any key of [`REFUSED_KEYS`];
//!    b. a `url` that is not a string, or names a scheme other than http, https or
//!    `about:blank`;
//!    c. an `extraArgs` that is not an array of strings each in [`EXTRA_ARGS_ALLOWED`];
//!    d. any other string, at any depth, that reads as a command-line flag (`-x`, `--x`,
//!    `--x=y`): agent-browser's MCP server puts typed values into the CLI's argv, and the CLI
//!    reads global flags anywhere in argv (`mcp.rs:2659-2661`).
//!
//!    A refused request gets an `isError` tool result with its id; a refused notification is
//!    dropped. Anything else is forwarded.

use serde_json::{Map, Value, json};

/// The longest agent line the guard reads, in bytes without the newline. A longer one is
/// refused as a parse error and never forwarded.
pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Argument keys refused on every tool, matched exactly as agent-browser matches them
/// (`mcp.rs:2264-2308`). Session, namespace and idle time are Clauseo's; restore reads and writes
/// saved browser state; the CA fields change which certificates Chrome trusts; `headed` shows a
/// window; `screenshotDir` and `path` write files outside the state folder, past the agent's own
/// write permissions.
pub(crate) const REFUSED_KEYS: &[&str] = &[
    "session",
    "namespace",
    "idleTimeout",
    "restore",
    "restoreSave",
    "restoreCheckUrl",
    "restoreCheckText",
    "restoreCheckFn",
    "caCert",
    "useSystemCa",
    "clearCaCert",
    "headed",
    "screenshotDir",
    "path",
];

/// The only `extraArgs` tokens that pass, each exactly. None takes a value.
pub(crate) const EXTRA_ARGS_ALLOWED: &[&str] =
    &["--json", "--full", "-i", "--quiet", "-q", "--annotate"];

/// The text of a refused batch.
pub(crate) const BATCH_MESSAGE: &str = "Clauseo does not allow batched requests in the browser.";

/// The longest name a refusal repeats; a longer one is cut and ends in `...`.
const NAME_MAX_CHARS: usize = 64;

/// What becomes of one agent line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Send this line (no newline) to agent-browser.
    Forward(String),
    /// Send this line (no newline) back to the agent; nothing reaches agent-browser.
    Answer(String),
    /// Nothing either way: a refused notification.
    Drop,
}

/// Checks one line, without its newline.
pub(crate) fn check(line: &[u8]) -> Verdict {
    let Ok(message) = serde_json::from_slice::<Value>(line) else {
        return parse_error();
    };
    let Value::Object(object) = &message else {
        let text = if message.is_array() {
            BATCH_MESSAGE
        } else {
            "Invalid Request"
        };
        return Verdict::Answer(error_line(&Value::Null, -32600, text));
    };
    if let Err(name) = check_object(object) {
        return match object.get("id") {
            Some(id) => Verdict::Answer(refusal_line(id, &name)),
            None => Verdict::Drop,
        };
    }
    match serde_json::to_string(&message) {
        Ok(text) => Verdict::Forward(text),
        Err(error) => Verdict::Answer(error_line(&Value::Null, -32603, &error.to_string())),
    }
}

/// The answer to an unreadable or oversized line.
pub(crate) fn parse_error() -> Verdict {
    Verdict::Answer(error_line(&Value::Null, -32700, "Parse error"))
}

/// `Err` with the name a refusal shows.
fn check_object(object: &Map<String, Value>) -> Result<(), String> {
    if object.get("method").and_then(Value::as_str) != Some("tools/call") {
        return Ok(());
    }
    // Not an object (or absent): agent-browser refuses it before running anything
    // (`validate_arguments_object`, mcp.rs:3967-3975, reached from every tool's argument reads).
    let Some(arguments) = object
        .get("params")
        .and_then(|params| params.get("arguments"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    check_arguments(arguments)
}

fn check_arguments(arguments: &Map<String, Value>) -> Result<(), String> {
    if let Some(key) = REFUSED_KEYS
        .iter()
        .find(|key| arguments.contains_key(**key))
    {
        return Err((*key).to_owned());
    }
    if let Some(url) = arguments.get("url") {
        check_url(url)?;
    }
    if let Some(extra) = arguments.get("extraArgs") {
        check_extra_args(extra)?;
    }
    for (key, value) in arguments {
        if key != "extraArgs" {
            check_no_flags(value)?;
        }
    }
    Ok(())
}

/// http and https, `about:blank`, or no scheme at all (agent-browser then adds `https://`,
/// `cli/src/commands.rs:76-90`, and `read` does the same, `read.rs:151-165`).
fn check_url(url: &Value) -> Result<(), String> {
    let Some(text) = url.as_str() else {
        return Err("url".to_owned());
    };
    let lowered = text.trim().to_lowercase();
    if lowered.starts_with("http://")
        || lowered.starts_with("https://")
        || lowered == "about:blank"
        || !lowered.contains(':')
    {
        return Ok(());
    }
    let scheme = lowered.split(':').next().unwrap_or_default();
    Err(format!("{} URLs", scheme_name(scheme)))
}

/// The scheme as written when it is a well-formed one (RFC 3986: a letter, then letters, digits,
/// `+`, `-` or `.`), else a fixed word, so the refusal never echoes arbitrary text.
fn scheme_name(scheme: &str) -> &str {
    let mut chars = scheme.chars();
    let well_formed = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && scheme.len() <= 32;
    if well_formed { scheme } else { "non-web" }
}

fn check_extra_args(extra: &Value) -> Result<(), String> {
    let Some(tokens) = extra.as_array() else {
        return Err("extraArgs".to_owned());
    };
    for token in tokens {
        let Some(token) = token.as_str() else {
            return Err("extraArgs".to_owned());
        };
        if !EXTRA_ARGS_ALLOWED.contains(&token) {
            return Err(flag_name(token));
        }
    }
    Ok(())
}

/// Every string in `value`, at any depth, that the CLI could read as a flag.
fn check_no_flags(value: &Value) -> Result<(), String> {
    match value {
        Value::String(text) if looks_like_flag(text) => Err(flag_name(text)),
        Value::Array(items) => items.iter().try_for_each(check_no_flags),
        Value::Object(map) => map.values().try_for_each(check_no_flags),
        _ => Ok(()),
    }
}

/// `-` or `--` followed by an ASCII letter: the shape of every agent-browser flag. `-5`, `-`,
/// `---` and `- item` pass.
fn looks_like_flag(text: &str) -> bool {
    let rest = text.strip_prefix("--").or_else(|| text.strip_prefix('-'));
    rest.and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_alphabetic())
}

/// A token's name: the part before any `=`, cut to [`NAME_MAX_CHARS`].
fn flag_name(token: &str) -> String {
    let name = token.split('=').next().unwrap_or_default();
    if name.chars().count() <= NAME_MAX_CHARS {
        name.to_owned()
    } else {
        let mut cut: String = name.chars().take(NAME_MAX_CHARS).collect();
        cut.push_str("...");
        cut
    }
}

/// The refusal's text, for tests too.
pub(crate) fn refusal_text(name: &str) -> String {
    format!("Clauseo does not allow {name} in the browser.")
}

fn refusal_line(id: &Value, name: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": refusal_text(name)}],
            "isError": true,
        },
    })
    .to_string()
}

fn error_line(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

#[cfg(test)]
mod tests;
