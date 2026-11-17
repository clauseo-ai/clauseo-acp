//! The options a Codex permission request may offer the lawyer: an allow-list by codex-acp
//! 2.2.2's exact option ids ([`keep_option`]), so no card can make a rule that outlives the
//! conversation.
//!
//! Three of Codex's answers outlive the session, in the agent's home, which every session and
//! every matter of the agent shares (`agent/codex.rs`): codex rust-v0.162.1 appends a
//! `prefix_rule` for `ApprovedExecpolicyAmendment` and a `network_rule` for
//! `NetworkPolicyAmendment` to `$CODEX_HOME/rules/default.rules` (`core/src/exec_policy.rs:
//! 464-560`), and writes `mcp_servers.<s>.tools.<t>.approval_mode` to a `config.toml` for
//! `ApprovedMcpPolicyAmendment` (`core/src/mcp_tool_call.rs:2266-2412`). codex-acp offers them
//! as `accept_execpolicy_amendment`, `apply_network_policy_amendment:<n>` and the MCP
//! `allow_always` (`dist/index.js:32023-32041`, `:32186-32194`, `:32682-32689`). None of them is
//! on the list, nor is any id codex-acp may add later: an unknown id is dropped (fail closed).
//!
//! What stays: the once-only answers, the refusals, and the session-only answers
//! (`allow_for_session`, `allow_session`, `allow_permissions_session`). Codex keeps a
//! session answer in memory for the rest of the session only and replays it by key with no new
//! card (`core/src/tools/approvals.rs:239-278`, `sandboxing.rs:71-116` at rust-v0.162.1): an
//! escalated command's `ApprovedForSession` reruns the same command unsandboxed, and a file
//! change's skips the card for a later patch touching only paths already approved. That is what
//! Codex's own "for this session" answer does, and the card says it: a Codex card's
//! allow-always answer reads "For this conversation", never "Always for this matter"
//! (`ui/components/src/clauseo/permission.tsx` `scope`). A dropped option is not offered,
//! so an answer naming one is refused as one the agent did not offer
//! (`actor/turn/answers.rs`, `permission.unknown_option`), and the host never sends it to the
//! agent. The host's own answer to a guarded `browser` tool (`allow_session`,
//! `actor/turn/asks/codex_auto.rs`) is given before this filter and is not a card.

use clauseo_protocol::PermissionOption;

/// The codex-acp 2.2.2 option ids a Codex permission request keeps, by exact id: once-only and
/// session-only answers and refusals only.
/// - commands and file changes (`ApprovalOptionId`, `dist/index.js:32023-32034`): `allow_once`,
///   `allow_for_session`, `decline`, `cancel`;
/// - MCP tools and server questions (`McpApprovalOptionId`, `:32035-32041`, and the question's
///   `accept`, `:32669`): `allow_once`, `allow_session`, `accept`, `decline`, `cancel`;
/// - extra permissions (`:32228-32250`): `allow_permissions_turn`,
///   `allow_permissions_turn_strict_auto_review` (scope `turn`), `allow_permissions_session`,
///   `reject_permissions`;
/// - the plan review (`:39819-39820`): `implement_plan`, `revise_plan`.
pub(crate) const KEPT_OPTIONS: [&str; 12] = [
    "allow_once",
    "allow_for_session",
    "allow_session",
    "accept",
    "decline",
    "cancel",
    "allow_permissions_turn",
    "allow_permissions_turn_strict_auto_review",
    "allow_permissions_session",
    "reject_permissions",
    "implement_plan",
    "revise_plan",
];

/// Whether a Codex permission request keeps `option`: its id is one of [`KEPT_OPTIONS`],
/// exactly (case included, as codex-acp matches it).
pub(crate) fn keep_option(option: &PermissionOption) -> bool {
    KEPT_OPTIONS.contains(&option.option_id.as_str())
}

#[cfg(test)]
mod tests;
