//! A failed rename's or archive's commit puts the thread row back, all but what the session
//! wrote since.

use clauseo_protocol::{
    AgentChoiceId, AgentId, AgentSessionId, ConfigPicks, MatterId, ThreadId, Timestamp,
};
use clauseo_store::{OwnPicks, ThreadRow};
use uuid::Uuid;

use super::rolled_back;

#[test]
fn a_failed_commit_keeps_the_session_id_and_kept_picks_written_since() {
    let before = ThreadRow {
        id: ThreadId::new(Uuid::now_v7()).unwrap(),
        matter: MatterId::new(Uuid::now_v7()).unwrap(),
        agent: AgentId::new("codex-acp").unwrap(),
        title: Some("Old".to_owned()),
        title_source: None,
        acp_session_id: None,
        archived: false,
        created: Timestamp::ZERO,
        updated: Timestamp::ZERO,
        seen_seq: None,
        own_picks: Some(OwnPicks::default()),
    };
    let mut picks = ConfigPicks::default();
    let id = |text: &str| AgentChoiceId::new(text).unwrap();
    picks.set(id("model"), id("gpt-6-luna")).unwrap();
    let picks = OwnPicks([(before.agent.clone(), picks)].into());
    let now = ThreadRow {
        title: Some("New".to_owned()),
        archived: true,
        acp_session_id: Some(AgentSessionId::new("sess-1").unwrap()),
        own_picks: Some(picks.clone()),
        ..before.clone()
    };
    let row = rolled_back(before, &now);
    assert_eq!(row.title.as_deref(), Some("Old"));
    assert!(!row.archived);
    assert_eq!(row.acp_session_id, now.acp_session_id);
    assert_eq!(row.own_picks, Some(picks));
}
