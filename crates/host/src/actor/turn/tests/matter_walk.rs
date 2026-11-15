//! The matter's link walk at each turn's start (`matter_walk.rs`): with no new link the agent
//! keeps its matter rules and runs the next turn; a link out of the matter that appeared between
//! two turns, with no command of the agent's, starts the agent again before the next turn, and the
//! new agent has no matter rules.

use std::path::PathBuf;

use clauseo_protocol::{CommandOutcome, DetachReason, DomainEvent, PromptBlock, ThreadId, TurnId};
use clauseo_protocol::{HostCommand, StopReason};

use super::rig::{Rig, kind};
use crate::pipeline::tests::support::TempDir;

/// Two prompts end at once; a new agent plays them from the start.
const ANSWERS: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
"#;

fn send_prompt(rig: &mut Rig, n: u128, thread: ThreadId) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Read the brief.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    let accepted = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("not accepted: {accepted:?}");
    };
    turn
}

/// Handles inputs until `thread` has no turn and nothing is in flight.
fn until_idle(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none()
            && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
}

/// The attached agent's id, whether it has the matter's rules, and the turn it last walked for.
fn agent(rig: &Rig, thread: ThreadId) -> (u64, bool, Option<TurnId>) {
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let walked = agent.matter_rules.and_then(|walks| walks.walked_for);
    (agent.id, agent.matter_rules.is_some(), walked)
}

fn ended(log: &[DomainEvent], turn: TurnId) -> Option<StopReason> {
    log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            return Some(ended.stop);
        }
        None
    })
}

#[cfg(unix)]
#[test]
fn a_link_out_that_appears_between_turns_takes_the_matter_rules_away_at_the_next_turn() {
    let mut rig = Rig::start("matter-walk-turn", ANSWERS);
    let thread = rig.thread();
    let matter = rig.matter();
    let first = send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let (launched, rules, walked) = agent(&rig, thread);
    assert!(rules, "the launch gave the plain matter its rules");
    assert_eq!(walked, Some(first), "the launch's walk counts for its turn");
    assert_eq!(ended(&rig.log(thread), first), Some(StopReason::EndTurn));

    // No new link: the second turn's walk keeps the agent and its rules.
    let second = send_prompt(&mut rig, 11, thread);
    until_idle(&mut rig, thread);
    assert_eq!(agent(&rig, thread), (launched, true, Some(second)));
    let log = rig.log(thread);
    assert_eq!(ended(&log, second), Some(StopReason::EndTurn));
    assert!(!log.iter().any(|event| matches!(event,
        DomainEvent::AgentSessionDetached(detached) if detached.reason == DetachReason::Restarting)));

    // A link out of the matter appears with no command of the agent's (a zip unpacked).
    let outside = TempDir::new("matter-walk-outside");
    let folder = PathBuf::from(&rig.actor.matters[&matter].row.folder_path);
    crate::write::symlink(outside.path(), &folder.join("unpacked")).unwrap();
    let third = send_prompt(&mut rig, 12, thread);
    until_idle(&mut rig, thread);
    let (relaunched, rules, walked) = agent(&rig, thread);
    assert_ne!(relaunched, launched, "the agent was started again");
    assert!(!rules, "the new agent has no matter rules");
    assert_eq!(walked, None, "with no rules, nothing is walked");
    let log = rig.log(thread);
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    let restarted = log.iter().position(|event| {
        matches!(event,
        DomainEvent::AgentSessionDetached(detached) if detached.reason == DetachReason::Restarting)
    });
    let started = log.iter().position(|event| {
        matches!(event,
        DomainEvent::TurnStarted(started) if started.turn == third)
    });
    assert!(
        matches!((restarted, started), (Some(r), Some(s)) if r < s),
        "the agent was not started again before the turn: {kinds:?}"
    );
    assert_eq!(ended(&log, third), Some(StopReason::EndTurn), "{kinds:?}");
    rig.finish();
}
