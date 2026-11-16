//! How big Claude Code's auto-mode context would be for a launch's flag settings, estimated
//! from the settings alone: a pure function, no I/O.
//!
//! Claude Code 2.1.293 sends the auto-mode classifier's context (its JSON holds the session's
//! permission rules) with each request only while that JSON is at most [`WITHHELD_ABOVE`] bytes
//! (`Gen`: `Buffer.byteLength(JSON.stringify(ctx))`). Above it, the request goes without the
//! context and every `Edit` or `Bash` of the whole launch is refused with "no server verdict for
//! this call (server_not_requested)". Measured against a fake API (private
//! `hostfu/b4l-autoctx.md`): each rule is `{"rule":<Claude's spelling>,"source":…}`, that is
//! the rule's JSON with its backslashes doubled, plus 33 B; a `Read(…)` or `Edit(…)` rule also
//! carries a root and a pattern (about 225 B a rule in all, a shell rule about 108 B); `deny`
//! and `ask` rules count, `allow` rules count nothing in auto mode; and a few KB are fixed.
//!
//! A launch whose estimate is over [`BUDGET`] runs in ask mode instead of `auto`
//! (`actor/turn/session/ask_mode.rs`): no rule is dropped, and the launch is never refused.

use serde_json::{Map, Value};

/// The largest auto-mode context Claude Code 2.1.293 sends (`Gen`'s 1,040,000 bytes).
pub(crate) const WITHHELD_ABOVE: usize = 1_040_000;

/// The estimate past which a launch runs in ask mode: 900,000 bytes, about 87% of
/// [`WITHHELD_ABOVE`], so the estimate's error and what this file does not count (the
/// session's own text in the context) stay under the line.
pub(crate) const BUDGET: usize = 900_000;

/// The context's fixed part beside the rules (about 3 KB).
const FIXED: usize = 3_000;

/// Each rule's wrapper beside its JSON: `{"rule":…,"source":"flagSettings"}`.
const PER_RULE: usize = 33;

/// A file rule's root and pattern wrapper beside their JSON.
const PER_FILE_RULE: usize = 20;

/// The tools whose rules name a path, which Claude Code splits into a root and a pattern.
const FILE_TOOLS: [&str; 7] = [
    "Read",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "Glob",
    "Grep",
];

/// The settings' `permissions` lists that reach the context: `allow` is left out in auto mode.
const COUNTED: [&str; 2] = ["deny", "ask"];

/// The estimated auto-mode context, in bytes, for a launch whose flag settings (the object
/// Claude Code reads from `--settings`) are `settings`.
pub(crate) fn estimate(settings: &Map<String, Value>) -> usize {
    let Some(permissions) = settings.get("permissions").and_then(Value::as_object) else {
        return FIXED;
    };
    let rules = COUNTED
        .iter()
        .filter_map(|list| permissions.get(*list)?.as_array())
        .flatten()
        .filter_map(Value::as_str);
    FIXED + rules.map(rule_cost).sum::<usize>()
}

/// `meta`'s Claude flag settings: `claudeCode.options.settings`, while still an object (before
/// `launch_settings` moves it into a file).
pub(crate) fn settings_of(meta: &Map<String, Value>) -> Option<&Map<String, Value>> {
    meta.get("claudeCode")?
        .get("options")?
        .get("settings")?
        .as_object()
}

/// One rule's share of the context.
fn rule_cost(rule: &str) -> usize {
    let spelled = rule.replace('\\', "\\\\");
    let mut cost = json_len(&spelled) + PER_RULE;
    if let Some(content) = file_content(rule) {
        // `//c/Users/…` is the root `/` and the pattern `/c/Users/…`; any other path is taken
        // whole as the pattern under a one-character root.
        let pattern = if content.starts_with("//") {
            content.get(1..).unwrap_or(content)
        } else {
            content
        };
        cost += json_len("/") + json_len(&pattern.replace('\\', "\\\\")) + PER_FILE_RULE;
    }
    cost
}

/// The path of a file tool's rule (`Read(<path>)`), or `None` for any other rule.
fn file_content(rule: &str) -> Option<&str> {
    let (tool, rest) = rule.split_once('(')?;
    FILE_TOOLS
        .contains(&tool)
        .then(|| rest.strip_suffix(')'))
        .flatten()
}

/// `text`'s length as a JSON string.
fn json_len(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len() + 2, |json| json.len())
}

#[cfg(test)]
#[path = "auto_context_tests.rs"]
mod tests;
