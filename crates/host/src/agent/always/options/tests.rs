use clauseo_protocol::{AgentChoiceId, PermissionKind, PermissionOption};

use super::{ALWAYS, agent_choice, agent_options, once_only, with_always};

fn option(id: &str, kind: PermissionKind) -> PermissionOption {
    PermissionOption {
        option_id: id.into(),
        kind,
        agent_label: id.to_owned(),
    }
}

fn ids(options: &[PermissionOption]) -> Vec<&str> {
    options
        .iter()
        .map(|option| option.option_id.as_str())
        .collect()
}

#[test]
fn no_agent_option_that_keeps_anything_reaches_the_lawyer() {
    use PermissionKind::{AllowAlways, AllowOnce, RejectOnce};
    let codex_command = [
        option("allow_once", AllowOnce),
        option("allow_for_session", AllowAlways),
        option("accept_execpolicy_amendment", AllowAlways),
        option("apply_network_policy_amendment:0", AllowAlways),
        option("decline", RejectOnce),
        option("cancel", RejectOnce),
    ];
    let codex_mcp = [
        option("allow_once", AllowOnce),
        option("allow_session", AllowAlways),
        option("allow_always", AllowAlways),
        option("decline", RejectOnce),
    ];
    let claude = [
        option("allow-once", AllowOnce),
        option("allow-with-updates", AllowAlways),
        option("allow-skill-exact", AllowOnce),
        option("allow-skill-prefix", AllowOnce),
        option("clauseo.always", AllowAlways),
        option("clauseo.session:allow_once", AllowAlways),
        option("reject-once", RejectOnce),
    ];
    let cases: [(&[PermissionOption], &[&str]); 3] = [
        (&codex_command, &["allow_once", "decline", "cancel"]),
        (
            &codex_mcp,
            &["allow_once", "clauseo.session:allow_session", "decline"],
        ),
        (&claude, &["allow-once", "reject-once"]),
    ];
    for (offered, kept) in cases {
        let mut options = offered.to_vec();
        agent_options(&mut options);
        assert_eq!(ids(&options), kept);
        let always = options
            .iter()
            .filter(|option| option.kind == AllowAlways)
            .all(|option| option.option_id.starts_with("clauseo.session:"));
        assert!(always, "{options:?}");
        with_always(&mut options);
        assert_eq!(options.get(1).map(|o| o.option_id.as_str()), Some(ALWAYS));
    }
}

#[test]
fn a_plan_exit_passes_unchanged_but_for_host_ids() {
    use PermissionKind::{AllowAlways, RejectOnce};
    let mut options = vec![
        option("exit-plan-accept-edits", AllowAlways),
        option("Exit-Plan-default", AllowAlways),
        option("clauseo.always", AllowAlways),
        option("plan", RejectOnce),
    ];
    agent_options(&mut options);
    assert_eq!(
        ids(&options),
        ["exit-plan-accept-edits", "Exit-Plan-default", "plan"]
    );
}

#[test]
fn a_host_option_maps_back_to_the_agents_own() {
    let id = |text: &str| AgentChoiceId::new(text).unwrap();
    assert_eq!(agent_choice(id(ALWAYS)), None);
    assert_eq!(
        agent_choice(id("clauseo.session:allow_session")),
        Some(id("allow_session"))
    );
    assert_eq!(agent_choice(id("clauseo.other")), None);
    assert_eq!(agent_choice(id("allow_once")), Some(id("allow_once")));
}

/// A paid tool's card (case law's `recharge`) offers only this call: no session option, renamed
/// or not, and nothing of kind `allow_always`.
#[test]
fn a_paid_tool_offers_only_this_call() {
    use PermissionKind::{AllowAlways, AllowOnce, RejectOnce};
    let mut options = vec![
        option("allow_once", AllowOnce),
        option("allow_session", AllowAlways),
        option("clauseo.session:allow_session", AllowOnce),
        option("allow_always", AllowAlways),
        option("decline", RejectOnce),
    ];
    once_only(&mut options);
    assert_eq!(ids(&options), ["allow_once", "decline"]);
    assert!(crate::agent::always::spends("case-law", "recharge"));
    assert!(!crate::agent::always::spends("case-law", "search_law"));
}
