//! Forced ask's hook layer (`agent/forced_ask.rs`): with [`ASK_ALL_FLAG`] among its arguments
//! the guard, after its own refusals, answers `ask` for every tool but [`LOCAL_READS`], so
//! Claude Code asks the lawyer whatever its mode and whatever allow rules the user's own settings
//! hold. A subagent's tool calls run the same hook. `WebSearch` and `WebFetch` ask too: nothing
//! fences the network either. An input the guard cannot read, or one with no `tool_name`, asks
//! (asking blocks nothing). The answer is Claude Code's `PreToolUse` decision on stdout, exit 0.
//!
//! [`ASK_ALL_FLAG`]: crate::agent::forced_ask::ASK_ALL_FLAG

use std::ffi::OsString;

use serde_json::Value;

/// The tools that only read on this computer and change nothing: these alone go on unasked.
pub(crate) const LOCAL_READS: [&str; 7] = [
    "Read",
    "Glob",
    "Grep",
    "LS",
    "NotebookRead",
    "TodoWrite",
    "Task",
];

/// What Claude Code is told when it must ask.
const REASON: &str = "This matter's folder is unprotected (a network drive, or a drive without \
                      file permissions), so Clauseo asks the lawyer before every change.";

/// Whether `args` (after the guard's flag) ask for forced ask.
pub(super) fn wanted(args: &[OsString]) -> bool {
    args.iter()
        .any(|arg| arg == crate::agent::forced_ask::ASK_ALL_FLAG)
}

/// Whether the hook input `bytes` names a tool that must ask: any but [`LOCAL_READS`], and any
/// input whose tool cannot be read.
pub(super) fn asks(bytes: &[u8]) -> bool {
    let tool = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|hook| hook.get("tool_name")?.as_str().map(str::to_owned));
    tool.is_none_or(|tool| !LOCAL_READS.contains(&tool.as_str()))
}

/// The `PreToolUse` output that makes Claude Code ask, one line.
pub(super) fn decision() -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": REASON,
        }
    })
    .to_string()
}
