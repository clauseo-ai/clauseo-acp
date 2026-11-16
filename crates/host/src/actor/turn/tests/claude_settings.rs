//! A live Claude conversation stops when its checks fail (`prompt/claude_settings.rs`): the
//! user-writable policy appearing between two prompts fails the second before it is sent
//! (`agent.user_policy`); the policy appearing, or Claude's user settings file changing, while a
//! prompt runs ends that turn at the next poll (`agent.user_policy`, `agent.settings_changed`).
//! The user tier is played through `user_policy_for_tests`; the clock is the rig's.

use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentKind, DomainEvent, ErrorCode, StopReason, ThreadId, TurnId};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline_quiet::{END_TURN, PROMPT, chunk};
use super::rig::Rig;
use crate::actor::turn::TurnPhase;
use crate::actor::turn::prompt::claude_settings::POLL;
use crate::actor::turn::wide::UserTierRead;

/// An HKCU policy that applies and turns the sandbox off.
const POLICY: &str = r#"{"sandbox":{"enabled":false}}"#;

/// A Claude rig whose agent runs `lines`, with the user tier played by `tier` (none yet).
fn start(test: &str, lines: &[String]) -> (Rig, ThreadId, Arc<Mutex<UserTierRead>>) {
    let mut rig = Rig::start(test, &(lines.join("\n") + "\n"));
    rig.actor.turn_path.kind_for_tests = Some(AgentKind::Claude);
    let tier = Arc::new(Mutex::new(Ok(None)));
    rig.actor.turn_path.user_policy_for_tests = Some(Arc::clone(&tier));
    let thread = rig.thread();
    (rig, thread, tier)
}

/// `turn`'s stop and failure code in `thread`'s log.
fn end(rig: &mut Rig, thread: ThreadId, turn: TurnId) -> (Option<StopReason>, Option<ErrorCode>) {
    let log = rig.log(thread);
    let stop = log.iter().find_map(|event| {
        let DomainEvent::TurnEnded(ended) = event else {
            return None;
        };
        (ended.turn == turn).then_some(ended.stop)
    });
    let code = log.iter().find_map(|event| {
        let DomainEvent::TurnFailed(failed) = event else {
            return None;
        };
        (failed.turn == turn).then_some(failed.code)
    });
    (stop, code)
}

/// Sends prompt `n` and runs until it is out to the agent.
fn prompting(rig: &mut Rig, n: u128, thread: ThreadId) -> TurnId {
    let turn = send_prompt(rig, n, thread);
    rig.run_until("the prompt", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.phase == TurnPhase::Prompting)
    });
    turn
}

#[test]
fn a_user_policy_appearing_between_two_prompts_stops_the_second() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        END_TURN.to_owned(),
    ];
    let (mut rig, thread, tier) = start("claude-policy-between", &lines);
    let first = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        end(&mut rig, thread, first),
        (Some(StopReason::EndTurn), None)
    );
    *tier.lock().unwrap() = Ok(Some(POLICY.to_owned()));
    let second = send_prompt(&mut rig, 11, thread);
    until_ended(&mut rig, thread);
    let (stop, code) = end(&mut rig, thread, second);
    assert_eq!(stop, Some(StopReason::Failed));
    assert_eq!(code, Some(ErrorCode::AgentUserPolicy));
    rig.finish();
}

#[test]
fn a_user_policy_appearing_while_a_prompt_runs_stops_the_turn_at_the_next_poll() {
    let lines = [PROMPT.to_owned(), chunk("Working")];
    let (mut rig, thread, tier) = start("claude-policy-mid-turn", &lines);
    let turn = prompting(&mut rig, 10, thread);
    rig.clock.advance(POLL);
    rig.run_tasks_until_stalled();
    assert!(
        rig.actor.threads[&thread].turn.current.is_some(),
        "no policy: still running"
    );
    *tier.lock().unwrap() = Ok(Some(POLICY.to_owned()));
    rig.clock.advance(POLL);
    until_ended(&mut rig, thread);
    let (stop, code) = end(&mut rig, thread, turn);
    assert_eq!(stop, Some(StopReason::Failed));
    assert_eq!(code, Some(ErrorCode::AgentUserPolicy));
    rig.finish();
}

#[test]
fn claudes_settings_file_changing_while_a_prompt_runs_stops_the_turn_at_the_next_poll() {
    let lines = [PROMPT.to_owned(), chunk("Working")];
    let (mut rig, thread, _) = start("claude-settings-mid-turn", &lines);
    let turn = prompting(&mut rig, 10, thread);
    let home = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .and_then(|agent| agent.claude_home.clone())
        .expect("a Claude home");
    assert!(crate::agent::home::claude_settings_intact(&home));
    let file = home.join(crate::agent::home::CLAUDE_SETTINGS_FILE);
    crate::write::write_file(
        &file,
        br#"{"syncClaudeAiSkills":false,"env":{"IS_SANDBOX":"1"}}"#,
    )
    .unwrap();
    rig.clock.advance(POLL);
    until_ended(&mut rig, thread);
    let (stop, code) = end(&mut rig, thread, turn);
    assert_eq!(stop, Some(StopReason::Failed));
    assert_eq!(code, Some(ErrorCode::AgentSettingsChanged));
    rig.finish();
}
