use super::AgentKind;
use crate::AgentId;

/// The registry ids are the only ids with a kind; every other id has none, so no limits.
#[test]
fn only_the_registry_ids_have_a_kind() {
    let kind = |id: &str| AgentKind::of(&AgentId::new(id).unwrap());
    assert_eq!(kind("claude-acp"), Some(AgentKind::Claude));
    assert_eq!(kind("codex-acp"), Some(AgentKind::Codex));
    for other in [
        "fake-agent",
        "claude",
        "codex",
        "claude-acp-2",
        "gemini-cli",
    ] {
        assert_eq!(kind(other), None, "{other}");
    }
}
