//! What the independent review of C-3b found, each pinned: a helper's step held for the agent's
//! own turn still shows under its card when the helper's end is logged first; a helper spawned
//! as a Stop lands makes no card and gets its agent stopped.

use clauseo_acp::{HelperState, Origin};
use clauseo_protocol::{AgentKind, CommandOutcome, DeliveryRequest, HostCommand, PromptBlock};
use serde_json::Value;

use super::agents::send_prompt;
use super::codex_helpers::{Codex, LEASE, step, story};
use super::codex_stop::reloaded;
use super::codex_unended::first_turn_leaves_a_helper_open;
use super::failures::until_ended;
use super::helpers::{CANCEL, LEAD_STEP, PROMPT, lead_step_logged, one_turn};
use super::idle::{idle_stops, until_ended as until_agent_ended};
use super::rig::Rig;
use super::tools_live::rig_adjusted;
use crate::actor::input::Input;
use crate::actor::turn::own_turn;

/// The second prompt and its answer, after `one_turn`'s first.
const SECOND: &str = r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
"#;

/// A helper an earlier turn left open sends a step while no turn is live (a queued prompt's
/// turn waits for its checkpoint), so the step waits for that turn; its end comes next and is
/// logged at once. The step, logged when the turn starts, still carries the helper: its stamp
/// stays until the step is logged, whatever the helper's end did meanwhile.
#[test]
fn a_helpers_step_held_for_the_next_turn_shows_under_its_card_after_its_end() {
    let steps = format!("{}{SECOND}", one_turn("end_turn"));
    let mut rig = Rig::start("codex-held-step", &steps);
    let thread = rig.thread();
    first_turn_leaves_a_helper_open(&mut rig, thread);
    let prompt = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Now the notice.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
    };
    // The prompt's turn waits for its before checkpoint, so no turn is live.
    let accepted = rig
        .dispatch_holding(11, prompt, |_, input| matches!(input, Input::Matter { .. }))
        .unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("{accepted:?}");
    };
    let mut codex = Codex::new(&mut rig, thread, turn);
    codex.update(Origin::Idle, LEASE, step("exec-r1"));
    codex.ended(Origin::Idle, LEASE, HelperState::Completed);
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 1);
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "turn_ended EndTurn",
            "end Lease Done -",
            "step exec-r1 Lease",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

/// A Stop lands while the lead's spawn is in flight: the helper's `subagent_spawned` comes late
/// (after the cancel). It makes no card, and the agent, whose helper would work on unseen, is
/// stopped once the cancelled turn has closed; the next prompt relaunches it, loads the session
/// and is answered.
#[test]
fn a_helper_spawned_as_a_stop_lands_gets_its_agent_stopped_and_no_card() {
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let steps: Vec<Value> = [PROMPT, LEAD_STEP, CANCEL, respond]
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut rig = rig_adjusted("codex-late-spawn", &steps, &reloaded(), true, |c| c);
    rig.actor.turn_path.kind_for_tests = Some(AgentKind::Codex);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    let mut codex = Codex::new(&mut rig, thread, turn);
    codex.spawned(Origin::Late(turn), LEASE, None, "Lease");
    // The cancelled turn closes, then the agent is stopped.
    until_agent_ended(&mut rig, thread);
    assert_eq!(idle_stops(&mut rig, thread), 1, "the agent was not stopped");
    send_prompt(&mut rig, 12, thread);
    rig.run_until("the next turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    assert_eq!(
        story(&rig.log(thread)),
        ["step tc-0 -", "turn_ended Cancelled", "turn_ended EndTurn"]
    );
    rig.finish();
}
