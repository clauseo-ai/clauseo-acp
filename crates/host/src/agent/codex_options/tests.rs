use clauseo_protocol::{PermissionKind, PermissionOption};

use super::keep_option;

fn option(id: &str, kind: PermissionKind) -> PermissionOption {
    PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: id.to_owned(),
    }
}

/// The ids a request of `offered` keeps, in order.
fn kept(offered: &[(&str, PermissionKind)]) -> Vec<String> {
    offered
        .iter()
        .map(|(id, kind)| option(id, *kind))
        .filter(keep_option)
        .map(|option| option.option_id.to_string())
        .collect()
}

use PermissionKind::{AllowAlways, AllowOnce, RejectAlways, RejectOnce};

/// A command as Codex 0.162.1 offers it with a proposed prefix (the live run of 2026-11-16):
/// allow once and cancel stay; the prefix rule goes.
#[test]
fn a_command_loses_the_prefix_rule() {
    let offered = [
        ("allow_once", AllowOnce),
        ("accept_execpolicy_amendment", AllowAlways),
        ("cancel", RejectOnce),
    ];
    assert_eq!(kept(&offered), ["allow_once", "cancel"]);
}

/// codex-acp's fallback set for a command (`command-decision-contract.ts`, used when Codex
/// sends no `availableDecisions`): the session answer stays ("For this conversation"); the
/// prefix rule goes.
#[test]
fn a_command_keeps_its_session_option_and_loses_the_prefix_rule() {
    let offered = [
        ("allow_once", AllowOnce),
        ("allow_for_session", AllowAlways),
        ("accept_execpolicy_amendment", AllowAlways),
        ("decline", RejectOnce),
        ("cancel", RejectOnce),
    ];
    assert_eq!(
        kept(&offered),
        ["allow_once", "allow_for_session", "decline", "cancel"]
    );
}

/// A command with network amendments, allow and deny, and its session answer: the session
/// answer stays, neither amendment does.
#[test]
fn a_network_card_keeps_its_session_answer_and_neither_amendment() {
    let offered = [
        ("allow_once", AllowOnce),
        ("allow_for_session", AllowAlways),
        ("apply_network_policy_amendment:0", AllowAlways),
        ("apply_network_policy_amendment:1", RejectAlways),
        ("cancel", RejectOnce),
    ];
    assert_eq!(
        kept(&offered),
        ["allow_once", "allow_for_session", "cancel"]
    );
}

/// A file change keeps `allow_for_session`: Codex skips the card for later patches of the
/// same paths for the rest of the session only, as in plain Codex.
#[test]
fn a_file_change_keeps_its_session_option() {
    let offered = [
        ("allow_once", AllowOnce),
        ("allow_for_session", AllowAlways),
        ("cancel", RejectOnce),
    ];
    assert_eq!(
        kept(&offered),
        ["allow_once", "allow_for_session", "cancel"]
    );
}

/// An MCP tool loses `allow_always` (written to a `config.toml`) and keeps `allow_session`;
/// a server's own question keeps its own.
#[test]
fn an_mcp_tool_loses_allow_always_and_keeps_allow_session() {
    let offered = [
        ("allow_once", AllowOnce),
        ("allow_session", AllowAlways),
        ("allow_always", AllowAlways),
        ("cancel", RejectOnce),
    ];
    assert_eq!(kept(&offered), ["allow_once", "allow_session", "cancel"]);
    let offered = [
        ("accept", AllowOnce),
        ("allow_session", AllowAlways),
        ("allow_always", AllowAlways),
        ("decline", RejectOnce),
        ("cancel", RejectOnce),
    ];
    assert_eq!(
        kept(&offered),
        ["accept", "allow_session", "decline", "cancel"]
    );
}

/// Extra permissions keep the turn grants, the session grant and the refusal; the plan review
/// keeps both of its options.
#[test]
fn permissions_keep_the_session_grant_and_the_plan_review_keeps_its_own() {
    let offered = [
        ("allow_permissions_turn", AllowOnce),
        ("allow_permissions_turn_strict_auto_review", AllowOnce),
        ("allow_permissions_session", AllowAlways),
        ("reject_permissions", RejectOnce),
    ];
    assert_eq!(
        kept(&offered),
        [
            "allow_permissions_turn",
            "allow_permissions_turn_strict_auto_review",
            "allow_permissions_session",
            "reject_permissions"
        ]
    );
    let offered = [("implement_plan", AllowOnce), ("revise_plan", RejectOnce)];
    assert_eq!(kept(&offered), ["implement_plan", "revise_plan"]);
}

/// No answer that outlives the session survives: the prefix rule, the network rules and the
/// MCP `allow_always`, whatever kind the agent claims; the three session answers do.
#[test]
fn no_answer_that_outlives_the_session_is_kept() {
    for id in [
        "allow_for_session",
        "allow_session",
        "allow_permissions_session",
    ] {
        assert!(keep_option(&option(id, AllowAlways)), "{id}");
    }
    for id in [
        "accept_execpolicy_amendment",
        "apply_network_policy_amendment:0",
        "allow_always",
    ] {
        assert!(!keep_option(&option(id, AllowAlways)), "{id}");
        // Nor under another kind: the id decides, never the kind the agent claims.
        assert!(!keep_option(&option(id, AllowOnce)), "{id}");
    }
}

/// Fail closed: an id codex-acp may add later, a look-alike or a case variant is dropped,
/// whatever its kind.
#[test]
fn an_unknown_id_is_dropped() {
    let offered = [
        ("allow_always_for_matter", AllowAlways),
        ("allow_once_v2", AllowOnce),
        ("Allow_Once", AllowOnce),
        ("allow_once ", AllowOnce),
        ("apply_network_policy_amendment", AllowAlways),
        ("", RejectOnce),
    ];
    assert_eq!(kept(&offered), [""; 0]);
}
