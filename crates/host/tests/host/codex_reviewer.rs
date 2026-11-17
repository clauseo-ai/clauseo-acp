//! Codex's automatic mode refused by the account (`actor/turn/codex_reviewer.rs`): the prompt
//! the account refused goes again once in workspace-write, with one notice and no failure; the
//! agent's later sessions start in workspace-write; a second refusal and any other error are
//! the turn's, with no retry.

use clauseo_protocol::{DomainEvent, NoticeKind, StopReason, StreamId};

use crate::codex::{CODEX, run_turn, turn_failed};
use crate::observe::Events;
use crate::support::{Dir, Scenario, matter_with_thread};

/// The `AutoModeUnavailable` notices logged, by their host text.
fn auto_notices(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) if notice.kind == Some(NoticeKind::AutoModeUnavailable) => {
                Some(notice.text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The modes logged, in order.
fn modes(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ModeChanged(changed) => Some(changed.mode_id.to_string()),
            _ => None,
        })
        .collect()
}

const LINE: &str = "ChatGPT's automatic mode isn't available for this account, so ChatGPT asks before running commands.";

#[test]
fn a_refused_reviewer_resends_once_in_workspace_write_and_later_sessions_start_there() {
    let dir = Dir::new(
        "codex_reviewer_refused",
        &[(CODEX, Scenario::Local("codex-auto-review-refused"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert!(turn_failed(&events, turn).is_none(), "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Asked first.");
    assert_eq!(
        modes(&events),
        ["agent", "workspace-write"],
        "{}",
        events.dump()
    );
    assert_eq!(auto_notices(&events), [LINE], "{}", events.dump());
    // A second turn in the same conversation: no second notice.
    let second = run_turn(&host, &mut events, thread);
    let _ = second;
    assert_eq!(auto_notices(&events).len(), 1, "{}", events.dump());

    // Another conversation with the same agent: it starts in workspace-write, with its own notice.
    let (_, other, _) = matter_with_thread(&dir, &host, "n", &[("brief.txt", "y\n")], CODEX);
    let mut later = host.events(StreamId::Thread(other));
    let turn = run_turn(&host, &mut later, other);
    later.assert_no_mismatch();
    assert_eq!(later.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(later.turn_text(turn), "Asked first again.");
    assert_eq!(modes(&later), ["workspace-write"], "{}", later.dump());
    assert_eq!(auto_notices(&later), [LINE], "{}", later.dump());
    host.shutdown();
}

#[test]
fn a_second_refusal_fails_the_turn_with_no_second_resend() {
    let dir = Dir::new(
        "codex_reviewer_refused_twice",
        &[(CODEX, Scenario::Local("codex-auto-review-refused-twice"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert!(turn_failed(&events, turn).is_some(), "{}", events.dump());
    assert_eq!(
        modes(&events),
        ["agent", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn another_prompt_error_gets_no_retry_and_no_notice() {
    let dir = Dir::new(
        "codex_prompt_other_error",
        &[(CODEX, Scenario::Local("codex-prompt-other-error"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert!(turn_failed(&events, turn).is_some(), "{}", events.dump());
    assert_eq!(modes(&events), ["agent"], "{}", events.dump());
    assert!(auto_notices(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}
