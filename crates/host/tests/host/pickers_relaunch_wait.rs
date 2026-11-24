//! A relaunch whose session reports its list late or not at all (`prompt_picks.rs`,
//! `prompt_picks/send.rs`): a lawyer's pick made while step 4 waits wins over the thread's own
//! value, and its answer is the list (`pickers-codex-picked-waiting`); with no list by the
//! wait's end the notice names no value of the last session, and a value the list later lacks
//! gets no second notice (`pickers-codex-no-list`); a late round keeps the first round's loss
//! (`pickers-codex-late-round`); and a turn cancelled while it waits ends at once
//! (`pickers-codex-cancel-wait`).

use std::time::Duration;

use clauseo_protocol::{CommandOutcome, HostCommand, NoticeKind, StopReason};

use crate::codex::CODEX;
use crate::pickers::{current, set};
use crate::pickers_early::sent;
use crate::pickers_idle::{Stopped, header_options, started_and_notices, stopped};
use crate::support::{send_prompt, wait};

/// The thread's kept values.
const KEPT: [(&str, &str); 2] = [("model", "gpt-6-luna"), ("reasoning_effort", "low")];

/// Step 4's longest wait for the session's list (`send.rs`'s `PICKS_WAIT`).
const PICKS_WAIT: Duration = Duration::from_secs(20);

#[test]
fn a_pick_made_while_step_4_waits_wins_over_the_kept_value() {
    let scenario = "pickers-codex-picked-waiting";
    let Stopped {
        dir: _dir,
        host,
        thread,
        mut events,
        ..
    } = stopped((CODEX, &KEPT, None), "pickers_picked_waiting", scenario);
    let (second, _) = host.prompt(thread, "And clause 5?");
    // Step 4 waits for the session's list: the lawyer's pick, made now, answers with one.
    host.wait_armed(PICKS_WAIT);
    assert_eq!(host.ok(set(thread, "model", "gpt-6")), CommandOutcome::Done);
    let ended = events.until_turn_ended(second);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let shown = header_options(&host, thread);
    assert_eq!(current(&shown, "model").as_deref(), Some("gpt-6"));
    assert_eq!(current(&shown, "reasoning_effort").as_deref(), Some("low"));
    host.shutdown();
}

#[test]
fn with_no_list_by_the_waits_end_the_one_notice_names_no_old_value() {
    let scenario = "pickers-codex-no-list";
    let Stopped {
        dir: _dir,
        host,
        thread,
        mut events,
        ..
    } = stopped((CODEX, &KEPT, None), "pickers_no_list", scenario);
    let (second, _) = host.prompt(thread, "And clause 5?");
    host.wait_armed(PICKS_WAIT);
    host.clock.advance(PICKS_WAIT);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    let (third, _) = host.prompt(thread, "And clause 6?");
    let ended = events.until_turn_ended(third);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, (None, None), "{}", events.dump());
    let none = NoticeKind::PickNotUsed {
        model: None,
        effort: None,
    };
    assert_eq!(notices, [none], "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_late_round_keeps_the_first_rounds_lost_model() {
    let scenario = "pickers-codex-late-round";
    let Stopped {
        dir: _dir,
        host,
        folder,
        thread,
        mut events,
    } = stopped((CODEX, &KEPT, Some("300")), "pickers_late_round", scenario);
    let prompt = sent(&host, send_prompt(thread, "And clause 5?"));
    let cue = folder.join("prepared.txt");
    for _ in 0..12_000 {
        if cue.exists() {
            break;
        }
        wait("the cue", async_io::Timer::after(Duration::from_millis(5)));
    }
    assert!(cue.exists(), "the agent never reached session/set_mode");
    let pick = sent(&host, set(thread, "reasoning_effort", "high"));
    let CommandOutcome::PromptAccepted { turn, .. } = wait("the prompt", prompt).unwrap() else {
        panic!("SendPrompt answered another outcome");
    };
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(wait("the pick", pick).unwrap(), CommandOutcome::Done);
    let (_, notices) = started_and_notices(&events, turn);
    let told = NoticeKind::PickNotUsed {
        model: Some("GPT-6".to_owned()),
        effort: Some("High".to_owned()),
    };
    assert_eq!(notices, [told], "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_turn_cancelled_while_it_waits_for_the_list_ends_at_once() {
    let scenario = "pickers-codex-cancel-wait";
    let Stopped {
        dir: _dir,
        host,
        thread,
        mut events,
        ..
    } = stopped((CODEX, &KEPT, None), "pickers_cancel_wait", scenario);
    let (second, _) = host.prompt(thread, "And clause 5?");
    host.wait_armed(PICKS_WAIT);
    host.ok(HostCommand::Cancel { thread });
    let ended = events.until_turn_ended(second);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    host.shutdown();
}
