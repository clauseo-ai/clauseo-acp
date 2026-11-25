//! Codex's helpers through the turn path, put in by hand as the ACP reader hands them over
//! (`codex_helpers.rs` has the harness): the order the cards rely on between a helper's report
//! (`HelperAnswer`) and its end (`HelperEnded`); how each end state maps; a replay after
//! `session/load` and late frames; a step id the lead already has; and a helper still open when
//! the turn ends by itself.
//!
//! The order: the host relies on acp handing over a helper's report before its end, as acp's
//! own tests pin (`crates/acp/src/tests/codex_helpers.rs`,
//! `one_helper_comes_on_the_root_session`: the `final_answer` chunks, then the end). The end's
//! summary is the report kept until then; a report piece after the end changes nothing.

use clauseo_acp::{HelperState, Origin};
use clauseo_protocol::DomainEvent;

use super::codex_helpers::{Codex, LEASE, ORDER, RELAY, story, story_after, story_of};
use super::failures::until_ended;
use super::helpers::{live_turn, one_turn};
use super::rig::Rig;

/// A report that comes in pieces before the end is the end's summary, whole.
#[test]
fn a_codex_helpers_report_before_its_end_is_the_ends_summary() {
    let lines = story_of("codex-report-then-end", |codex| {
        let at = codex.at();
        codex.spawned(at, LEASE, None, "Lease");
        codex.model(LEASE, "gpt-6-luna");
        codex.answer(LEASE, "Term 1 April 2024");
        codex.answer(LEASE, " to 31 March 2026");
        codex.ended(at, LEASE, HelperState::Completed);
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "end Lease Done Term 1 April 2024 to 31 March 2026",
            "turn_ended EndTurn",
        ]
    );
}

/// An end with no report before it ends the card with none, and a report that comes after the
/// end, in the turn or late, changes nothing: no second end, no summary, no words.
#[test]
fn a_codex_helpers_end_without_a_report_has_none_and_a_later_report_changes_nothing() {
    let lines = story_of("codex-end-then-report", |codex| {
        let at = codex.at();
        codex.spawned(at, LEASE, None, "Lease");
        codex.model(LEASE, "gpt-6-luna");
        codex.ended(at, LEASE, HelperState::Completed);
        codex.answer(LEASE, "Term 1 April 2024");
        codex.origin = Origin::Late(codex.turn);
        codex.answer(LEASE, " to 31 March 2026");
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "end Lease Done -",
            "turn_ended EndTurn",
        ]
    );
}

/// `failed` ends the card `Failed`, `cancelled` and `disconnected` `Stopped`, each with the
/// report so far. A helper never heard from before its end starts at its end, without a model.
#[test]
fn each_codex_end_state_maps_to_its_outcome() {
    let lines = story_of("codex-end-states", |codex| {
        let at = codex.at();
        codex.spawned(at, LEASE, None, "Lease");
        codex.spawned(at, ORDER, None, "Order");
        codex.spawned(at, RELAY, None, "Relay");
        codex.answer(LEASE, "Rent is");
        codex.answer(ORDER, "Order of 9 May");
        codex.ended(at, LEASE, HelperState::Failed);
        codex.ended(at, ORDER, HelperState::Cancelled);
        codex.ended(at, RELAY, HelperState::Disconnected);
    });
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=- parent=-",
            "start Order model=- parent=-",
            "end Lease Failed Rent is",
            "end Order Stopped Order of 9 May",
            "start Relay model=- parent=-",
            "end Relay Stopped -",
            "turn_ended EndTurn",
        ]
    );
}

/// After `session/load` codex-acp replays each helper of the earlier process, ending it
/// `disconnected` (F1d-restart.jsonl:60-62): no card starts or ends. A late spawn makes no card;
/// a late report and a late end of an open helper change nothing, so it stays open.
#[test]
fn a_replayed_or_late_codex_helper_frame_changes_no_card() {
    let (lines, open) = story_after(
        "codex-helpers-replay",
        |codex| {
            let at = codex.at();
            let late = Origin::Late(codex.turn);
            codex.spawned(Origin::Replay, LEASE, None, "Lease");
            let options = super::codex_helpers::options("gpt-6-luna");
            codex.update(Origin::Replay, LEASE, options);
            codex.ended(Origin::Replay, LEASE, HelperState::Disconnected);
            codex.spawned(late, ORDER, None, "Order");
            codex.spawned(at, RELAY, None, "Relay");
            codex.model(RELAY, "gpt-6-luna");
            codex.origin = late;
            codex.answer(RELAY, "3 matches");
            codex.ended(late, RELAY, HelperState::Completed);
        },
        |_| {},
    );
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Relay model=gpt-6-luna parent=-",
            "turn_ended EndTurn",
        ]
    );
    assert_eq!(open, 1, "the helper is still open");
}

/// A Codex helper is a background helper: a turn that ends by itself leaves it open (one still
/// waiting for its model starts in the turn's close, without one), and its own end, after the
/// turn, ends it with its report.
#[test]
fn a_codex_helper_open_at_the_turns_end_stays_open_until_its_own_end() {
    let (lines, open) = story_after(
        "codex-helper-open-at-end",
        |codex| {
            let at = codex.at();
            codex.spawned(at, LEASE, None, "Lease");
            codex.spawned(at, ORDER, None, "Order");
            codex.model(LEASE, "gpt-6-luna");
        },
        |codex| {
            let idle = codex.at();
            codex.answer(ORDER, "Order of 9 May");
            codex.ended(idle, ORDER, HelperState::Completed);
            codex.ended(idle, LEASE, HelperState::Completed);
        },
    );
    assert_eq!(
        lines,
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "start Order model=- parent=-",
            "turn_ended EndTurn",
            "end Order Done Order of 9 May",
            "end Lease Done -",
        ]
    );
    assert_eq!(open, 0);
}

/// A helper's step whose id the lead's step already has is dropped with its updates: the lead's
/// step keeps its own status.
#[test]
fn a_codex_helper_step_with_a_lead_steps_id_leaves_the_lead_step_alone() {
    let mut rig = Rig::start("codex-helper-clash", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let mut codex = Codex::new(&mut rig, thread, turn);
    let at = codex.at();
    codex.spawned(at, LEASE, None, "Lease");
    codex.model(LEASE, "gpt-6-luna");
    codex.step(LEASE, "tc-0");
    codex.ended(at, LEASE, HelperState::Completed);
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    // The lead's step changes only when the turn's close settles it, after the helper's end.
    let before_end = log
        .iter()
        .take_while(|event| !matches!(event, DomainEvent::HelperEnded(_)));
    let patched = before_end
        .clone()
        .any(|event| matches!(event, DomainEvent::ToolCallUpdated(_)));
    assert!(!patched, "{:?}", story(&log));
    assert_eq!(
        story(&log),
        [
            "step tc-0 -",
            "start Lease model=gpt-6-luna parent=-",
            "end Lease Done -",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}
