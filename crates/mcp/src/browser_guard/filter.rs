//! The check on each line the agent sends to agent-browser. It sees only that direction; what
//! agent-browser sends back is relayed as read (`relay.rs`), but a `tools/list` reply, which
//! loses the tools refused here ([`only_allowed_tools`]).
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
//! 4. An object whose `method` is exactly `tools/call` naming a tool outside [`ALLOWED_TOOLS`]
//!    (a missing or non-string name included): a live view of [`LIVE_VIEW_TOOLS`] gets
//!    [`UNAVAILABLE`], any other [`UNKNOWN_TOOL`]; nothing forwarded. `agent_browser_batch` is
//!    not on the list.
//! 5. An object whose `method` is exactly `tools/call`, with `params.arguments` an object:
//!    a. any key of [`REFUSED_KEYS`];
//!    b. a key of [`MUST_BE_FALSE`] whose value is anything but `false`;
//!    c. a `timeoutMs` that is not a whole number up to [`MAX_TIMEOUT_MS`];
//!    d. on `agent_browser_screenshot`, a `selector` agent-browser would take for a file path;
//!    on `agent_browser_read`, any key of [`READ_FETCH_KEYS`];
//!    e. a `url` that is not a string, names a scheme other than http, https or
//!    `about:blank`, or names a local or private host (`host.rs`);
//!    f. an `extraArgs` that is not an array of strings each in [`EXTRA_ARGS_ALLOWED`];
//!    g. any other string, at any depth, shaped like a command-line flag (`-x`, `--x`,
//!    `--x=y`, see [`looks_like_flag`]): agent-browser's MCP server puts typed values into the
//!    CLI's argv, and the CLI reads global flags anywhere in argv (`mcp.rs:2659-2661`).
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
const TOOL_READ: &str = "agent_browser_read";

/// The keys on `agent_browser_read` that make agent-browser's daemon fetch over HTTP itself
/// rather than read the open tab's page through Chrome (v0.41.1 `native/actions.rs:5964-5978`:
/// `url` fetches that URL; `llms` and `requireMd` re-fetch the active tab's URL and its
/// `llms.txt` files, following redirects, `read.rs:198-215`). agent-browser marks `read`
/// read-only (`mcp.rs:2180-2186`), so Codex runs it with no prompt: refused, `read` reads only
/// the page already open, through `handle_content` (`actions.rs:5980-5994`).
pub(crate) const READ_FETCH_KEYS: &[&str] = &["url", "llms", "requireMd"];
/// The refusal of a [`READ_FETCH_KEYS`] key.
pub(crate) const READ_FETCH: &str =
    "agent_browser_read fetching a page itself (open the page, then read it)";
/// The refusal's name for a screenshot selector that reads as a path.
pub(crate) const SCREENSHOT_PATH: &str = "a file path as the screenshot selector";
/// The refusal's name for a `timeoutMs` out of range.
pub(crate) const TIMEOUT: &str = "a timeoutMs other than a whole number up to 300000";

/// The only `extraArgs` tokens that pass, each exactly. None takes a value.
pub(crate) const EXTRA_ARGS_ALLOWED: &[&str] =
    &["--json", "--full", "-i", "--quiet", "-q", "--annotate"];

/// The text of a refused batch.
pub(crate) const BATCH_MESSAGE: &str = "Clauseo does not allow batched requests in the browser.";

/// The only tools that reach agent-browser: agent-browser v0.41.1's `core` profile
/// (`CORE_PROFILE_TOOLS`, `cli/src/mcp.rs:340-370` at tag v0.41.1), the profile the host starts
/// it with (`mcp --tools core`, `crates/profile/src/servers.rs:81`), less `agent_browser_eval`:
/// page JavaScript is off for both agents, so it is refused as any other tool off the list, and
/// dropped from agent-browser's `tools/list` reply ([`only_allowed_tools`]). An agent that starts
/// the guard itself with `--tools all` gains nothing: every other tool, a new or renamed one
/// included, is refused here. `agent_browser_tools_profiles` only prints the profiles
/// (`call_tools_profiles`, `mcp.rs:2539-2560`); it changes none.
pub(crate) const ALLOWED_TOOLS: &[&str] = &[
    "agent_browser_tools_profiles",
    "agent_browser_open",
    "agent_browser_read",
    "agent_browser_snapshot",
    "agent_browser_back",
    "agent_browser_forward",
    "agent_browser_reload",
    "agent_browser_click",
    "agent_browser_fill",
    "agent_browser_type",
    "agent_browser_press",
    "agent_browser_check",
    "agent_browser_uncheck",
    "agent_browser_select",
    "agent_browser_scroll",
    "agent_browser_wait_ms",
    "agent_browser_wait_for_selector",
    "agent_browser_wait_for_text",
    "agent_browser_wait_for_load",
    "agent_browser_screenshot",
    "agent_browser_get_text",
    "agent_browser_get_url",
    "agent_browser_get_title",
    "agent_browser_tab_new",
    "agent_browser_tab_list",
    "agent_browser_tab_switch",
    "agent_browser_tab_close",
    "agent_browser_close",
];

/// A reply to `tools/list` (`line`, without its newline) with every tool off [`ALLOWED_TOOLS`]
/// dropped from `result.tools` (`agent_browser_eval` in the core profile), so the agent never
/// sees a tool the guard refuses; an entry with no string name stays. `None` when `line` is no
/// such reply or drops nothing: it then goes on as read. The relay calls it only for the reply to
/// a `tools/list` request (`relay/pumps.rs`, `relay/oneshot.rs`).
pub(crate) fn only_allowed_tools(line: &[u8]) -> Option<Vec<u8>> {
    let mut reply: Value = serde_json::from_slice(line).ok()?;
    let tools = reply.get_mut("result")?.get_mut("tools")?.as_array_mut()?;
    let before = tools.len();
    tools.retain(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_none_or(|name| ALLOWED_TOOLS.contains(&name))
    });
    if tools.len() == before {
        return None;
    }
    serde_json::to_vec(&reply).ok()
}

/// The words in agent-browser's `core` profile description (`ToolProfile::description`,
/// `cli/src/mcp.rs:257` at v0.41.1) that name the eval tool the guard refuses.
const EVAL_WORDS: &str = "JavaScript eval, ";

/// A reply to `agent_browser_tools_profiles` (`line`, without its newline) with
/// [`EVAL_WORDS`] taken out of every string in it, the text content and any structured content
/// alike, so the agent is never told its tools include JavaScript eval. `None` when nothing
/// changes: the line then goes on as read.
pub(crate) fn without_eval(line: &[u8]) -> Option<Vec<u8>> {
    fn strip(value: &mut Value) -> bool {
        match value {
            Value::String(text) if text.contains(EVAL_WORDS) => {
                *text = text.replace(EVAL_WORDS, "");
                true
            }
            // Every item, not up to the first change.
            Value::Array(items) => {
                items
                    .iter_mut()
                    .map(strip)
                    .filter(|changed| *changed)
                    .count()
                    > 0
            }
            Value::Object(map) => {
                map.values_mut()
                    .map(strip)
                    .filter(|changed| *changed)
                    .count()
                    > 0
            }
            _ => false,
        }
    }
    let mut reply: Value = serde_json::from_slice(line).ok()?;
    strip(reply.get_mut("result")?).then(|| serde_json::to_vec(&reply).ok())?
}

/// agent-browser's live views, none of them on [`ALLOWED_TOOLS`], named only so their refusal
/// reads as a feature Clauseo leaves out: each serves the browser on a loopback port, the stream
/// server (`stream_enable`, `actions.rs:9772-9785` at v0.41.1, with its `disable` and
/// `status`), the dashboard that shows its streams, and the developer tools page of `inspect`
/// (`native/inspect_server.rs`). In the plain client the guard runs outside any sandbox, so this
/// refusal is what keeps them off.
pub(crate) const LIVE_VIEW_TOOLS: &[&str] = &[
    "agent_browser_stream_enable",
    "agent_browser_stream_disable",
    "agent_browser_stream_status",
    "agent_browser_dashboard_start",
    "agent_browser_dashboard_stop",
    "agent_browser_inspect",
];

/// What a live-view request gets: no path, no port.
pub(crate) const UNAVAILABLE: &str = "That browser feature isn't available in Clauseo.";

/// What a call to any other tool off [`ALLOWED_TOOLS`] gets, its name not repeated.
pub(crate) const UNKNOWN_TOOL: &str = "That browser tool isn't one Clauseo offers.";

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
    if let Err(text) = check_object(object) {
        return match object.get("id") {
            Some(id) => Verdict::Answer(refusal_line(id, &text)),
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

/// `Err` with the text of the refusal, whole.
fn check_object(object: &Map<String, Value>) -> Result<(), String> {
    if object.get("method").and_then(Value::as_str) != Some("tools/call") {
        return Ok(());
    }
    let params = object.get("params");
    let tool = params
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !ALLOWED_TOOLS.contains(&tool) {
        let text = if LIVE_VIEW_TOOLS.contains(&tool) {
            UNAVAILABLE
        } else {
            UNKNOWN_TOOL
        };
        return Err(text.to_owned());
    }
    // Not an object (or absent): agent-browser refuses it before running anything
    // (`validate_arguments_object`, mcp.rs:3967-3975, reached from every tool's argument reads).
    let Some(arguments) = params
        .and_then(|params| params.get("arguments"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    check_arguments(tool, arguments).map_err(|name| refusal_text(&name))
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
    if tool == TOOL_READ
        && READ_FETCH_KEYS
            .iter()
            .any(|key| arguments.contains_key(*key))
    {
        return Err(READ_FETCH.to_owned());
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

fn refusal_line(id: &Value, text: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}],
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
