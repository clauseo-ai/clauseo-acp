//! What the independent review of C-3b found, each pinned: a helper's step held for the agent's
//! own turn still shows under its card when the helper's end is logged first; a helper spawned
//! as a Stop lands makes no card and gets its agent stopped; a helper that fails to start ends
//! its card failed while the lead's turn goes on; a prompt queued after a Stop that leaves a
//! helper running, before the stopped turn has closed, goes once to the relaunched agent, after
//! its `session/load`, whichever comes back first, its commit or the turn's close; a stopped
//! helper keeps the report it had written so far.

use clauseo_acp::{HelperState, Origin};
use clauseo_protocol::{
    AgentKind, CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, PromptBlock,
};
use serde_json::Value;

use super::agents::send_prompt;
use super::codex_helpers::{Codex, LEASE, ORDER, step, story, story_of};
use super::codex_stop::{codex_rig, reloaded, until_the_step};
use super::codex_unended::first_turn_leaves_a_helper_open;
use super::failures::until_ended;
use super::helpers::{CANCEL, LEAD_STEP, PROMPT, lead_step_logged, one_turn};
use super::idle::{idle_stops, until_ended as until_agent_ended};
use super::rig::Rig;
use super::tools_live::rig_adjusted;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{AgentReply, TurnInput, own_turn};

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
        agent: None,
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

/// A helper that fails to start: no recording has one, and the one shape acp can hand over is
/// `HelperSpawned` then `HelperEnded { failed }` with nothing between (codex-acp marks a helper
/// `failed` when its wait times out or the turn fails; a spawn Codex refuses is swallowed with
/// its call in this mode, so no card starts). The card starts at its end, without a model, and
/// ends `Failed` with no report; the lead's turn goes on to its own end.
#[test]
fn a_codex_helper_that_fails_to_start_ends_failed_and_the_turn_goes_on() {
    let lines = story_of("codex-failed-start", |codex| {
        let at = codex.at();
        codex.spawned(at, LEASE, None, "Lease");
        codex.ended(at, LEASE, HelperState::Failed);
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=- parent=-",
            "end Lease Failed -",
            "turn_ended EndTurn",
        ]
    );
}

/// The agent's answer to a `session/prompt`.
fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// A Stop that leaves a helper running, then a prompt queued before the stopped turn has
/// closed. `close_first` picks which comes back first: the turn's close (the agent has
/// answered the cancel, the end's commit is on its way when the prompt is sent) or the
/// prompt's own commit (the agent's answer to the cancel waits until that commit is back).
/// Either way the prompt is the lawyer's new send after the Stop: the queue is not held, and
/// the prompt goes once to the relaunched agent, after its `session/load` (the relaunch's
/// script answers one prompt only).
fn queued_after_the_stop(test: &str, close_first: bool) {
    let (mut rig, thread) = codex_rig(test, true);
    until_the_step(&mut rig, thread);
    let stop = HostCommand::Cancel { thread };
    rig.dispatch_holding(11, stop, prompt_answer).unwrap();
    let prompt = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "And the notice.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
        agent: None,
    };
    if close_first {
        rig.release_held();
        rig.run_until("the stopped turn's end on its way", |actor| {
            actor.threads[&thread].turn.closing.is_some()
        });
        rig.dispatch(12, prompt).unwrap();
        let closing = rig.actor.threads[&thread].turn.closing.is_some();
        assert!(!closing, "the close came back before the prompt's commit");
    } else {
        rig.dispatch_holding(12, prompt, prompt_answer).unwrap();
        rig.run_holding("the queued prompt's commit", prompt_answer, |actor| {
            actor.work.commits == 0
        });
        let state = &rig.actor.threads[&thread].turn;
        assert!(state.current.is_some(), "the stopped turn has not closed");
        assert!(!state.queue.is_empty(), "the prompt waits behind it");
        rig.release_held();
    }
    rig.run_until("both turns' ends", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.queue.is_empty() && actor.work.commits == 0
    });
    let log = rig.log(thread);
    assert_eq!(
        story(&log),
        [
            "start Read documents model=gpt-6-luna parent=-",
            "step exec-sleep Read documents",
            "end Read documents Stopped -",
            "turn_ended Cancelled",
            "turn_ended EndTurn",
        ]
    );
    let prompts = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::UserMessage(_)))
        .count();
    assert_eq!(
        prompts, 2,
        "the Stopped prompt and the queued one, each once"
    );
    rig.finish();
}

/// The prompt's commit comes back while the stopped turn is still open. The close used to hold
/// the queue then, with nothing left to lift the hold (5 of 12 runs on Linux).
#[test]
fn a_prompt_queued_after_a_helpers_running_stop_starts_when_its_commit_is_back_first() {
    queued_after_the_stop("codex-stop-queued-commit", false);
}

/// The stopped turn's close comes back before the prompt's commit.
#[test]
fn a_prompt_queued_after_a_helpers_running_stop_starts_when_the_close_is_back_first() {
    queued_after_the_stop("codex-stop-queued-close", true);
}

/// A Stop while two helpers work: one had streamed two pieces of its final answer, the other
/// none. The cancel's own write ends both `Stopped`: the first with the pieces it had written as
/// its summary, the second with none (live run: a stopped helper's 2,204 characters were lost).
#[test]
fn a_stopped_codex_helper_keeps_the_report_written_so_far() {
    let respond = r#"{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;
    let steps = format!("{PROMPT}\n{LEAD_STEP}\n{CANCEL}\n{respond}\n");
    let mut rig = Rig::start("codex-stop-report", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the lead's step", |actor| lead_step_logged(actor, thread));
    let mut codex = Codex::new(&mut rig, thread, turn);
    let at = codex.at();
    codex.spawned(at, LEASE, None, "Lease");
    codex.spawned(at, ORDER, None, "Order");
    codex.model(LEASE, "gpt-6-luna");
    codex.model(ORDER, "gpt-6-luna");
    codex.answer(LEASE, "Term 1 April 2024");
    codex.answer(LEASE, " to 31 March 2026");
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "start Order model=gpt-6-luna parent=-",
            "end Lease Stopped Term 1 April 2024 to 31 March 2026",
            "end Order Stopped -",
            "turn_ended Cancelled",
        ]
    );
    rig.finish();
}
