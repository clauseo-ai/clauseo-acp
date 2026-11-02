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
//! 3. Any message whose `id` is [`RESERVED_ID`], the guard's own close request's: `-32600`
//!    with that id, nothing forwarded.
//! 4. An object whose `method` is exactly `tools/call`, with `params.arguments` an object:
//!    a. any key of [`REFUSED_KEYS`];
//!    b. a key of [`MUST_BE_FALSE`] whose value is anything but `false`;
//!    c. a `timeoutMs` that is not a whole number up to [`MAX_TIMEOUT_MS`];
//!    d. on `agent_browser_screenshot`, a `selector` agent-browser would take for a file path;
//!    e. a `url` that is not a string, names a scheme other than http, https or
//!    `about:blank`, or names a local or private host (`host.rs`);
//!    f. an `extraArgs` that is not an array of strings each in [`EXTRA_ARGS_ALLOWED`];
//!    g. any other string, at any depth, shaped like a command-line flag (`-x`, `--x`,
//!    `--x=y`, see [`looks_like_flag`]): agent-browser's MCP server puts typed values into the
//!    CLI's argv, and the CLI reads global flags anywhere in argv (`mcp.rs:2659-2661`).
//!    `agent_browser_eval`'s `script` is exempt: it travels on stdin (`mcp.rs:3950-3957`).
//!
//!    A refused request gets an `isError` tool result with its id; a refused notification is
//!    dropped. Anything else is forwarded.

mod host;

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

/// Argument keys refused on every tool unless their value is `false`. `newTab` makes
/// agent-browser open a link's `href` through CDP, past the `url` check and Chrome's block on
/// pages opening local files (`actions.rs:6619-6689`); `webgpu` adds `--enable-unsafe-webgpu`
/// (`chrome.rs:483-486`).
pub(crate) const MUST_BE_FALSE: &[&str] = &["newTab", "webgpu"];

/// The longest `timeoutMs` that passes: the MCP server runs one call at a time
/// (`mcp.rs:586`), so a huge one would stall it and the guard's shutdown.
pub(crate) const MAX_TIMEOUT_MS: u64 = 300_000;

/// The id of the guard's own close request (`relay.rs`); an agent message using it is refused,
/// so its reply can never be taken for the close reply.
pub(crate) const RESERVED_ID: &str = "clauseo-guard-close";

/// The tool whose single positional may be read as a file path (`commands.rs:912-933`).
const TOOL_SCREENSHOT: &str = "agent_browser_screenshot";
/// The tool whose `script` goes to agent-browser on stdin, never into argv.
const TOOL_EVAL: &str = "agent_browser_eval";
/// The refusal's name for a screenshot selector that reads as a path.
pub(crate) const SCREENSHOT_PATH: &str = "a file path as the screenshot selector";
/// The refusal's name for a `timeoutMs` out of range.
pub(crate) const TIMEOUT: &str = "a timeoutMs other than a whole number up to 300000";

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
    if let Some(id) = object
        .get("id")
        .filter(|id| id.as_str() == Some(RESERVED_ID))
    {
        let text = format!("Clauseo reserves the id {RESERVED_ID} for itself.");
        return Verdict::Answer(error_line(id, -32600, &text));
    }
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
    let params = object.get("params");
    let Some(arguments) = params
        .and_then(|params| params.get("arguments"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    let tool = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    check_arguments(tool, arguments)
}

fn check_arguments(tool: &str, arguments: &Map<String, Value>) -> Result<(), String> {
    if let Some(key) = REFUSED_KEYS
        .iter()
        .find(|key| arguments.contains_key(**key))
    {
        return Err((*key).to_owned());
    }
    if let Some(key) = MUST_BE_FALSE.iter().find(|key| {
        arguments
            .get(**key)
            .is_some_and(|v| *v != Value::Bool(false))
    }) {
        return Err((*key).to_owned());
    }
    if let Some(timeout) = arguments.get("timeoutMs")
        && timeout.as_u64().is_none_or(|ms| ms > MAX_TIMEOUT_MS)
    {
        return Err(TIMEOUT.to_owned());
    }
    if tool == TOOL_SCREENSHOT
        && let Some(selector) = arguments.get("selector").and_then(Value::as_str)
        && reads_as_path(selector)
    {
        return Err(SCREENSHOT_PATH.to_owned());
    }
    if let Some(url) = arguments.get("url") {
        check_url(url)?;
    }
    if let Some(extra) = arguments.get("extraArgs") {
        check_extra_args(extra)?;
    }
    for (key, value) in arguments {
        let exempt = key == "extraArgs" || (tool == TOOL_EVAL && key == "script");
        if !exempt {
            check_no_flags(value)?;
        }
    }
    Ok(())
}

/// Stricter than agent-browser's own test (`commands.rs:912-933`), which takes a lone
/// positional for a path when it holds `/` or ends in an image extension, case-sensitively,
/// unless it starts with `.`, `#` or `@`: here any `/` or `\`, or those endings in any case.
fn reads_as_path(selector: &str) -> bool {
    let lowered = selector.to_ascii_lowercase();
    selector.contains(['/', '\\'])
        || [".png", ".jpg", ".jpeg", ".webp"]
            .iter()
            .any(|ending| lowered.ends_with(ending))
}

/// http and https, `about:blank`, or no scheme at all (agent-browser then adds `https://`,
/// `cli/src/commands.rs:76-90`, and `read` does the same, `read.rs:151-165`); then a host that
/// is not local or private (`host.rs`).
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
        return host::check(text).map_err(str::to_owned);
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

/// The shape of every agent-browser flag: `-` or `--`, a name of ASCII letters, digits and
/// inner dashes that starts with a letter and does not end in a dash, then optionally `=` and
/// anything. `-5`, `-10`, `-`, `--`, `---x`, `- item`, `--Select--` and `-Select-` pass;
/// `--headed`, `-p`, `--proxy=x` and `-Rohan` do not.
fn looks_like_flag(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("--").or_else(|| text.strip_prefix('-')) else {
        return false;
    };
    let name = rest
        .split_once('=')
        .map_or(rest, |(name, _)| name)
        .as_bytes();
    name.first().is_some_and(u8::is_ascii_alphabetic)
        && name.last().is_some_and(u8::is_ascii_alphanumeric)
        && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
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
