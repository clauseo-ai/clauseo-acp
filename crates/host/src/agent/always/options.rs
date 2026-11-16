//! The options a permission card offers the lawyer (decision record 0063, "Options the host
//! sends"). An agent option that would write a rule, or claims a scope it does not have, is
//! dropped on arrival ([`agent_options`]); two host options take their place:
//!
//! - [`ALWAYS`] (kind `allow_always`), "Always for this matter", added when the request gives a
//!   rule the host keeps ([`with_always`]). The agent gets the request's allow-once option.
//! - [`SESSION`]`<agent id>`: an agent's session-only option (codex-acp's MCP `allow_session`),
//!   "For this conversation". The agent gets its own id back ([`agent_choice`]).
//!
//! Plan-exit options pass unchanged. An option id an agent sends that starts `clauseo.` is
//! always dropped, so no agent can name a host option.

use clauseo_protocol::{AgentChoiceId, PermissionKind, PermissionOption};

/// The host's "Always for this matter".
pub(crate) const ALWAYS: &str = "clauseo.always";

/// The prefix of the host's name for an agent's session-only option.
pub(crate) const SESSION: &str = "clauseo.session:";

/// The prefix every host option id has.
const HOST: &str = "clauseo.";

/// The plan exit's option ids start so (`agent/forced_ask.rs` `SKIPPING_OPTIONS`), in any case.
const PLAN_EXIT_OPTION: &str = "exit-plan-";

/// Agent options that would keep something beyond this one call, whatever their kind:
/// codex-acp 2.2.2's command amendment, session approval and turn grant (`allow_permissions_turn`,
/// kind `allow_once`: more permissions for the rest of the turn), claude-agent-acp 0.89.1's
/// rule-writing and skill options. Network amendments match by prefix ([`NETWORK_AMENDMENT`]).
const DROPPED: &[&str] = &[
    "accept_execpolicy_amendment",
    "allow_permissions_turn",
    "allow_for_session",
    "allow_always",
    "allow-always",
    "allow-with-updates",
    "allow-skill-exact",
    "allow-skill-prefix",
];

/// codex-acp's network-policy options, `apply_network_policy_amendment:<n>`.
const NETWORK_AMENDMENT: &str = "apply_network_policy_amendment";

/// Session-only agent options the host offers under [`SESSION`]: codex-acp's MCP
/// `allow_session` ("for this conversation").
const SESSION_ONLY: &[&str] = &["allow_session"];

/// Whether `options` hold a plan exit's (an `exit-plan-*` id, any case).
pub(crate) fn plan_exit(options: &[PermissionOption]) -> bool {
    options.iter().any(|option| {
        let id = option.option_id.as_bytes();
        id.get(..PLAN_EXIT_OPTION.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(PLAN_EXIT_OPTION.as_bytes()))
    })
}

/// `options` without any an agent sent under a host id (`clauseo.*`): first thing on arrival,
/// before any other filter, so no agent can offer `clauseo.always` itself.
pub(crate) fn drop_host_ids(options: &mut Vec<PermissionOption>) {
    options.retain(|option| !option.option_id.starts_with(HOST));
}

/// The agent's `options` as the lawyer may see them: every `clauseo.` id dropped; then, unless
/// it is a plan exit, the rule-writing ones ([`DROPPED`], network amendments, and any other of
/// kind `allow_always`) dropped and the session-only ones renamed [`SESSION`]`<id>`.
pub(crate) fn agent_options(options: &mut Vec<PermissionOption>) {
    drop_host_ids(options);
    if plan_exit(options) {
        return;
    }
    options.retain_mut(|option| {
        let id = option.option_id.as_str();
        if SESSION_ONLY.contains(&id) {
            let renamed = format!("{SESSION}{id}");
            if AgentChoiceId::new(&renamed).is_err() {
                return false;
            }
            option.option_id = renamed.as_str().into();
            // Its own kind, not the agent's `allow_always`: it keeps nothing past this
            // conversation, and the app must not word it "Always for this matter".
            option.kind = PermissionKind::AllowSession;
            return true;
        }
        let dropped = DROPPED.contains(&id) || id.starts_with(NETWORK_AMENDMENT);
        !dropped && option.kind != PermissionKind::AllowAlways
    });
}

/// Whether `options` hold an agent's session-only option ([`SESSION_ONLY`]), as it sent it.
pub(crate) fn session_only(options: &[PermissionOption]) -> bool {
    options
        .iter()
        .any(|option| SESSION_ONLY.contains(&option.option_id.as_str()))
}

/// `options` without any that would allow more than this one call: for a tool that spends the
/// lawyer's money (`always::spends`), whose every call asks. Drops the session-only ones
/// ([`SESSION_ONLY`], and any already renamed [`SESSION`]) and every `allow_always` one.
pub(crate) fn once_only(options: &mut Vec<PermissionOption>) {
    options.retain(|option| {
        let id = option.option_id.as_str();
        !SESSION_ONLY.contains(&id)
            && !id.starts_with(SESSION)
            && !DROPPED.contains(&id)
            && option.kind != PermissionKind::AllowAlways
    });
}

/// `options` with [`ALWAYS`] after the first allow-once option (or first, with none).
pub(crate) fn with_always(options: &mut Vec<PermissionOption>) {
    if options.iter().any(|option| option.option_id == ALWAYS) {
        return;
    }
    let at = options
        .iter()
        .position(|option| option.kind == PermissionKind::AllowOnce)
        .map_or(0, |at| at + 1);
    let always = PermissionOption {
        option_id: ALWAYS.into(),
        kind: PermissionKind::AllowAlways,
        agent_label: "Always for this matter".to_owned(),
    };
    options.insert(at, always);
}

/// The option id the agent gets for the lawyer's `chosen` one: a [`SESSION`] option's own id;
/// none for [`ALWAYS`] (the turn path sends the allow-once option); else `chosen` itself.
pub(crate) fn agent_choice(chosen: AgentChoiceId) -> Option<AgentChoiceId> {
    let id = chosen.as_str();
    if id == ALWAYS {
        return None;
    }
    match id.strip_prefix(SESSION) {
        Some(own) => AgentChoiceId::new(own).ok(),
        None if id.starts_with(HOST) => None,
        None => Some(chosen),
    }
}

#[cfg(test)]
mod tests;
