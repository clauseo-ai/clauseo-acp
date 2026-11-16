//! `allowedDomains`, refused in every spelling before anything reaches agent-browser, and the
//! way back when agent-browser refuses it anyway.
//!
//! agent-browser turns `allowedDomains` into `--allowed-domains` (`append_common_global_args`,
//! `cli/src/mcp.rs:4167-4180` at v0.41.1), which it cannot apply to a Chrome it attached to over
//! CDP: the launch fails with "--allowed-domains is not supported with --cdp because WebRTC
//! containment cannot be installed before existing page scripts run"
//! (`native/actions.rs:3797-3801`), and every later call of that agent-browser session failed
//! the same way, a plain `open` included. Clauseo already decides which sites the browser
//! reaches, so the guard refuses the key outright ([`names_allowed_domains`]); should
//! agent-browser still answer with that error ([`domain_restricted`]), the guard stops the
//! session as its idle stop does, keeping the profile, so the next call starts clean
//! (`relay/pumps.rs`). It does so once per run: a page cannot make agent-browser's own error
//! field hold that sentence (a page's thrown error comes back as "Evaluation error: ..."), and
//! the limit bounds what a way past that could cost. A later one is answered with [`STUCK`].

use serde_json::{Map, Value, json};

/// The refusal of `allowedDomains`, in any spelling.
pub(crate) const ALLOWED_DOMAINS: &str = "Clauseo already controls which sites the browser may \
                                          open; open the page without allowedDomains.";

/// What the agent gets in place of agent-browser's domain-restriction error, once the guard has
/// stopped the session.
pub(crate) const RESTARTED: &str = "Clauseo already controls which sites the browser may open; \
                                    open the page without allowedDomains. Clauseo restarted the \
                                    browser, so the next call starts clean.";

/// What the agent gets in place of that error once the guard has used its one restart: what to
/// do, since an `agent_browser_close` reply always stops the browser (`relay.rs`).
pub(crate) const STUCK: &str = "The browser is stuck on a site restriction; close it with \
                                agent_browser_close, then open the page again without \
                                allowedDomains. Clauseo already controls which sites the \
                                browser may open.";

/// agent-browser v0.41.1's refusal of `--allowed-domains` on a Chrome it reached over CDP
/// (`native/actions.rs:3797-3801`), whole. Run against the release binary with
/// `AGENT_BROWSER_CDP` set, every call after an `allowedDomains` one, `agent_browser_close`
/// included, is answered with exactly this in `structuredContent.response.error`.
pub(crate) const CDP_REFUSAL: &str = "--allowed-domains is not supported with --cdp because \
                                      WebRTC containment cannot be installed before existing \
                                      page scripts run";

/// Whether `name` spells `allowedDomains`: leading dashes dropped, letter case, `-` and `_`
/// ignored, so `allowed_domains`, `allowed-domains`, `--allowed-domains` and `AllowedDomains`
/// all do.
fn spells_allowed_domains(name: &str) -> bool {
    let bare: String = name
        .trim_start_matches('-')
        .chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect();
    bare == "alloweddomains"
}

/// Whether `arguments` name `allowedDomains` anywhere: as a key at any depth, or as a string
/// at any depth, `extraArgs` included, that starts with `-` and spells it before any `=`
/// (`--allowed-domains`, `--allowed-domains=x`), as agent-browser's CLI would read the global
/// flag (`flags.rs:1172`).
pub(crate) fn names_allowed_domains(arguments: &Map<String, Value>) -> bool {
    fn in_value(value: &Value) -> bool {
        match value {
            Value::String(text) => {
                text.starts_with('-')
                    && spells_allowed_domains(text.split('=').next().unwrap_or_default())
            }
            Value::Array(items) => items.iter().any(in_value),
            Value::Object(map) => in_map(map),
            _ => false,
        }
    }
    fn in_map(map: &Map<String, Value>) -> bool {
        map.iter()
            .any(|(key, value)| spells_allowed_domains(key) || in_value(value))
    }
    in_map(arguments)
}

/// Whether `line`, a reply from agent-browser, is its refusal of `--allowed-domains`: its
/// error field (`result.structuredContent.response.error`, where agent-browser's MCP server puts
/// the CLI's error, or a JSON-RPC `error.message`) is exactly [`CDP_REFUSAL`]. Text that merely
/// holds the sentence (a page's title or thrown error, "Evaluation error: ...") does not count.
/// A byte search runs first, so a line without the sentence is never parsed.
pub(crate) fn domain_restricted(line: &[u8]) -> bool {
    let needle = CDP_REFUSAL.as_bytes();
    if !line.windows(needle.len()).any(|window| window == needle) {
        return false;
    }
    let Ok(reply) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    let said = |pointer: &str| reply.pointer(pointer).and_then(Value::as_str) == Some(CDP_REFUSAL);
    said("/result/structuredContent/response/error") || said("/error/message")
}

/// The reply that replaces agent-browser's domain-restriction error to request `id`: an
/// `isError` result with `text` ([`RESTARTED`] or [`STUCK`]). No newline.
pub(crate) fn replacement(id: &Value, text: &str) -> Vec<u8> {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": text}], "isError": true},
    })
    .to_string()
    .into_bytes()
}
