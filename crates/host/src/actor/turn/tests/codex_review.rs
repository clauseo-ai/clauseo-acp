//! What the independent review of C-3b found, each pinned: a helper's step held for the agent's
//! own turn still shows under its card when the helper's end is logged first.

use clauseo_acp::{HelperState, Origin};
use clauseo_protocol::{CommandOutcome, DeliveryRequest, HostCommand, PromptBlock};

use super::codex_helpers::{Codex, LEASE, step, story};
use super::codex_unended::first_turn_leaves_a_helper_open;
use super::failures::until_ended;
use super::helpers::one_turn;
use super::rig::Rig;
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
