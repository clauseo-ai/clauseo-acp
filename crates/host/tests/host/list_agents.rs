//! `ListAgents`: each agent of the settings with a valid id, with its kind.

use clauseo_protocol::{AgentKind, HostQuery, QueryResult};

use crate::codex::CODEX;
use crate::support::{Dir, FAKE, Scenario};

#[test]
fn list_agents_answers_each_valid_id_with_its_kind() {
    let dir = Dir::new(
        "list_agents",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (CODEX, Scenario::Bundled("codex-handshake")),
            ("claude-acp", Scenario::Bundled("prompt-stream")),
            ("Bad_ID", Scenario::Bundled("prompt-stream")),
        ],
    );
    let host = dir.start(0);
    let QueryResult::Agents(agents) = host.query(HostQuery::ListAgents).unwrap() else {
        panic!("ListAgents answered another result");
    };
    let listed: Vec<(&str, Option<AgentKind>)> =
        agents.iter().map(|a| (a.id.as_str(), a.kind)).collect();
    assert_eq!(
        listed,
        [
            ("claude-acp", Some(AgentKind::Claude)),
            (CODEX, Some(AgentKind::Codex)),
            (FAKE, None),
        ]
    );
    host.shutdown();
}
