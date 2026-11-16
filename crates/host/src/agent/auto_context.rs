//! Claude Code 2.1.293's auto-mode context bytes, estimated from a launch's flag settings alone:
//! the single measure for every decision about how many permission rules a launch may carry.
//! A pure function ([`estimate`]), no I/O.
//!
//! Claude Code 2.1.293 sends the auto-mode classifier's context (its JSON holds the session's
//! permission rules) with each request only while that JSON is at most [`CONTEXT_LIMIT`] bytes
//! (`Gen`: `Buffer.byteLength(JSON.stringify(ctx))`). Above it, the request goes without the
//! context and every `Edit` or `Bash` of the whole launch is refused with "no server verdict for
//! this call (`server_not_requested`)" (the server-verdict limit). A launch whose estimate is
//! over [`ASK_MODE_AT`] runs in ask mode instead of `auto` (`actor/turn/session/ask_mode.rs`):
//! no rule is dropped, and the launch is never refused.
//!
//! This is not `agent::loads::profile::SHED_AT`, a different limit: that one weighs the settings
//! **file** (its JSON bytes) against the 2 MiB Claude Code reads (`agent::launch_settings`), and
//! sheds the data folder's variable-form shell rules near it. Settings bytes are never compared
//! with the context limit: a rule costs about twice its settings bytes in the context.
//!
//! # The factors, and how they were measured
//!
//! Measured on Claude Code 2.1.293 (`@anthropic-ai/claude-agent-sdk` 0.3.293's binary, the pin
//! in `crates/limits/tests/live/package-lock.json`) against a fake Anthropic API that logged the
//! request's `safeguards[0].classifier_context`, bisecting the rule count with one tool use per
//! run (private `hostfu/b4l-autoctx.md`, 2026-11-16). A new Claude Code version means measuring
//! again; the test `the_factors_were_measured_on_the_pinned_claude_code` fails when the pin moves.
//!
//! - Each rule is `{"rule":<Claude's spelling>,"source":"flagSettings"}`: the rule's JSON with
//!   its backslashes doubled, plus 33 B ([`PER_RULE`]). A `Bash(…)` or `PowerShell(…)` rule is
//!   about 108 B in all.
//! - A `Read(…)` or `Edit(…)` rule also carries a root and a pattern (`//c/Users/…` is the root
//!   `/` and the pattern `/c/Users/…`), 20 B of wrapper beside their JSON ([`PER_FILE_RULE`]):
//!   about 225 B in all.
//! - `deny` and `ask` rules count; `allow` rules count nothing in auto mode.
//! - About 3 KB are fixed ([`FIXED`]).
//! - Calibration: main's Windows launch in a laptop-like folder at 8 matters sent 372,259 B
//!   (3,704 deny rules), #1085's at 200 matters 943,153 B (8,441); the estimate is within 5% of
//!   both (`auto_context_tests.rs`: +3.3% and +2.7%). The last rule set sent was 1,039,865 B.

use serde_json::{Map, Value};

/// The largest auto-mode context Claude Code 2.1.293 sends (`Gen`'s 1,040,000 bytes): above it,
/// no server verdict, and every `Edit` and `Bash` of the launch is refused.
pub(crate) const CONTEXT_LIMIT: usize = 1_040_000;

/// The estimate past which a launch runs in ask mode: 900,000 bytes, about 87% of
/// [`CONTEXT_LIMIT`], so the estimate's error and what it does not count stay under the line.
pub const ASK_MODE_AT: usize = 900_000;

/// The estimate at which a turn that ends on a blip of the auto-mode checks (a "gave no verdict"
/// that is not the withheld kind) goes to ask mode instead of a plain notice: 832,000 bytes, 80%
/// of [`CONTEXT_LIMIT`], where the limit may have moved under the estimate's error.
pub const ASK_ON_BLIP_AT: usize = 832_000;

const _: () = assert!(ASK_ON_BLIP_AT < ASK_MODE_AT && ASK_MODE_AT < CONTEXT_LIMIT);

/// The most a launch in the laptop-like folder at 8 matters may estimate (half the limit), for
/// the rule-size tests.
#[cfg(test)]
pub(crate) const TEST_BUDGET_AT_8: usize = 520_000;

/// The most a launch in the laptop-like folder at 200 matters may estimate ([`ASK_MODE_AT`]),
/// for the rule-size tests.
#[cfg(test)]
pub(crate) const TEST_BUDGET_AT_200: usize = 900_000;

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

/// Claude Code 2.1.293's auto-mode context bytes, the server-verdict limit being
/// [`CONTEXT_LIMIT`] (1,040,000), for a launch whose flag settings (the object Claude Code reads
/// from `--settings`) are `settings`. The factors are in the module doc.
pub fn estimate(settings: &Map<String, Value>) -> usize {
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
