//! Helpers from recorded frames played through the ACP reader (fake_agent and the files in
//! `recorded/`, taken from live runs of claude-agent-acp 0.110.1 on Claude Code 2.1.330, the
//! matter folder put as `{{cwd}}`): a background helper that hands back; two foreground helpers
//! at once, each handing back while its launch call only points at the handback; a helper cut at
//! its turn limit, then resumed, whose later steps stay under its ended card. And, by hand, a
//! launch call whose pointer comes before the handback lands.

use std::collections::HashMap;

use clauseo_acp::Origin;
use clauseo_protocol::{CompactString, DomainEvent, HelperId, TextOrBlob};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{
    COMPLETED, DELIVERED, PROMPT, call, lead, live_turn, one_turn, patch, stamped, story,
};
use super::rig::Rig;

const END_TURN: &str = r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#;

/// One prompt playing `frames`, then answered `end_turn`.
fn played(frames: &str) -> String {
    format!("{PROMPT}\n{frames}{END_TURN}\n")
}

/// Each step's ACP id with the ACP id of the helper it names (empty for the lead's).
fn steps_by_helper(log: &[DomainEvent]) -> Vec<(String, String)> {
    let mut helpers: HashMap<HelperId, CompactString> = HashMap::new();
    let mut steps = Vec::new();
    for event in log {
        match event {
            DomainEvent::HelperStarted(started) => {
                helpers.insert(started.helper, started.acp_id.clone());
            }
            DomainEvent::ToolCallStarted(started) => {
                let helper = started
                    .helper
                    .and_then(|helper| helpers.get(&helper))
                    .map(ToString::to_string)
                    .unwrap_or_default();
                steps.push((started.acp_id.to_string(), helper));
            }
            _ => {}
        }
    }
    steps
}

/// Each helper end's outcome and summary text (`<blob>` for a blob), in log order.
fn ends(log: &[DomainEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|event| match event {
            DomainEvent::HelperEnded(ended) => {
                let summary = match &ended.summary {
                    Some(TextOrBlob::Inline(text)) => text.clone(),
                    Some(TextOrBlob::Blob(_)) => "<blob>".to_owned(),
                    None => "-".to_owned(),
                };
                Some(format!("{:?} {summary}", ended.outcome))
            }
            _ => None,
        })
        .collect()
}

/// The recorded frames themselves (fake_agent's `claude-helper-background` scenario) through
/// the ACP reader: the helper starts from the `Agent` call's input, takes both stamped `Read`
/// steps and ends `Done` with its handback's digest; the handback is no step.
#[test]
fn the_recorded_background_helper_ends_on_its_handback() {
    let scenario =
        include_str!("../../../../../fake_agent/scenarios/claude-helper-background.jsonl");
    // The scenario's own header and handshake: the rig plays its own handshake.
    let steps: String = scenario
        .lines()
        .skip(2)
        .map(|line| format!("{line}\n"))
        .collect();
    let mut rig = Rig::start("helpers-recorded", &steps);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        story(&rig.log(thread)),
        [
            "step toolu_01Gr3CDaJEng8pyzihUmv3Cq -",
            "start paper-reader call=toolu_01Gr3CDaJEng8pyzihUmv3Cq model=haiku \
             label=Digest lease and notice task=true",
            "step toolu_01WesqZa8jMTM4WMn2ELQNTp paper-reader",
            "step toolu_01VFDnvL4EbFFj1FnZthBh4g paper-reader",
            // The digest is past half the inline limit, so it is logged as a blob.
            "end paper-reader Done <blob>",
            "turn_ended EndTurn",
        ]
    );
    rig.finish();
}

const A: &str = "toolu_01S38uffy4aWXr4DLaULZWX9";
const B: &str = "toolu_01Xbs85pormEoj1qQxg2VLuQ";

/// Two foreground helpers at once (live run L3): their steps interleave, each stamped with its
/// own launch call; each ends `Done` by its handback, and the launch calls' results, which only
/// point at the handbacks, become no summary.
#[test]
fn two_foreground_helpers_end_on_their_handbacks_not_their_pointers() {
    let frames = include_str!("recorded/claude-two-foreground.jsonl");
    let mut rig = Rig::start("helpers-recorded-two", &played(frames));
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let pair = |step: &str, helper: &str| (step.to_owned(), helper.to_owned());
    assert_eq!(
        steps_by_helper(&log),
        [
            pair(A, ""),
            pair(B, ""),
            pair("toolu_016Zynkx3gZd6Mx7rzUb5aoW", B),
            pair("toolu_01P5ADQo6W3KDT4croRWv4ym", A),
            pair("toolu_012B58wt2FVM8TTJKj3UYLAP", B),
            pair("toolu_01278qikm3joxXLsNYqEsfnr", A),
        ]
    );
    // Both digests are past half the inline limit: logged as blobs, never the pointers' text.
    assert_eq!(ends(&log), ["Done <blob>", "Done <blob>"]);
    rig.finish();
}

/// The launch call's pointer comes before its handback lands, as by hand: the helper stays open
/// until the handback, which ends it with the report.
#[test]
fn a_pointer_before_the_handback_lands_waits_for_the_handback() {
    let pointer = "This agent's report was delivered to you as a message from \"a1\" (its \
                   SubagentHandback call). Read it there; it is not repeated here.";
    let mut rig = Rig::start("helpers-pointer-first", &one_turn("end_turn"));
    let thread = rig.thread();
    let turn = live_turn(&mut rig, thread);
    let at = Origin::Turn(turn);
    let input = r#"{"subagent_type":"paper-reader","prompt":"Read."}"#;
    lead(
        &mut rig,
        thread,
        turn,
        call("toolu_A", "Agent", Some(input)),
    );
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_A", None, COMPLETED, Some(pointer)),
    );
    let handback = call(
        "toolu_H",
        "SubagentHandback",
        Some(r#"{"message":"Report."}"#),
    );
    stamped(&mut rig, thread, at, "toolu_A", handback);
    lead(
        &mut rig,
        thread,
        turn,
        patch("toolu_H", None, COMPLETED, Some(DELIVERED)),
    );
    rig.release_held();
    until_ended(&mut rig, thread);
    assert_eq!(ends(&rig.log(thread)), ["Done Report."]);
    rig.finish();
}

/// A helper cut at its turn limit (live run L4): its launch call completes with the limit's note
/// and no handback, so it ends `Done` with that note. The lead resumes it with `SendMessage`, and
/// the steps that follow, stamped with the same launch call, show under the ended helper without
/// opening it again.
#[test]
fn a_helper_cut_at_its_turn_limit_ends_done_and_keeps_its_later_steps() {
    let frames = include_str!("recorded/claude-turn-limit.jsonl");
    let mut rig = Rig::start("helpers-recorded-limit", &played(frames));
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let launch = "toolu_013m2ngiiU7751tyFvz2AWE4";
    let pair = |step: &str, helper: &str| (step.to_owned(), helper.to_owned());
    assert_eq!(
        steps_by_helper(&log),
        [
            pair(launch, ""),
            pair("toolu_01SSejkY7AUZHhrFAMAcdtaN", launch),
            pair("toolu_01HdvyAqwbPtwx7Pt4rtGP2e", ""),
            pair("toolu_01FPBv2qSoB8VGDVwnrLmQBH", launch),
            pair("toolu_01SRD2LkWcVAto7m1SciSrgR", launch),
        ]
    );
    let ends = ends(&log);
    let [end] = ends.as_slice() else {
        panic!("{ends:?}");
    };
    assert!(
        end.starts_with("Done [Agent stopped at its turn limit"),
        "{end}"
    );
    let starts = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::HelperStarted(_)))
        .count();
    assert_eq!(starts, 1);
    rig.finish();
}
