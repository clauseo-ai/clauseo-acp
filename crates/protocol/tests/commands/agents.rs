//! `list_agents` and its answer. `AgentKind::of` is tested in `src/limits/tests.rs`.

use clauseo_protocol::{AgentId, AgentKind, AgentSummary, HostQuery, QueryResult};

use super::fixtures::round_trips;

#[test]
fn list_agents_is_a_plain_string_and_answers_agents() {
    assert_eq!(
        serde_json::to_string(&HostQuery::ListAgents).unwrap(),
        r#""list_agents""#
    );
    round_trips(&HostQuery::ListAgents);
    let answer = QueryResult::Agents(vec![
        AgentSummary {
            id: AgentId::new("codex-acp").unwrap(),
            kind: Some(AgentKind::Codex),
        },
        AgentSummary {
            id: AgentId::new("fake-agent").unwrap(),
            kind: None,
        },
    ]);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"agents":[{"id":"codex-acp","kind":"codex"},{"id":"fake-agent","kind":null}]}"#
    );
    round_trips(&answer);
    // An older reader's summary without `kind` still reads, as no kind.
    let bare: AgentSummary = serde_json::from_str(r#"{"id":"claude-acp"}"#).unwrap();
    assert_eq!(bare.kind, None);
}
