//! A Codex MCP server that did not start (codex-acp 2.10.0 reports it as a failed `tool_call`
//! titled `mcp__<name>__startup`) is no step of the lawyer's: not when it comes right after the
//! `session/new` answer, before any prompt, nor in the middle of a turn, nor its update. The
//! lawyer gets one `tool_not_started` notice per server instead, naming the tool as they know it.
//! The scenarios also check that `session/new` carries the start-up wait.

use clauseo_protocol::{DomainEvent, NoticeKind, NoticeLevel, StopReason, StreamId, TurnId};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

const CODEX: &str = "codex-acp";

/// A host whose Codex agent plays `scenario`, after one prompt's turn and the shutdown: every
/// event of the thread, and the turn.
fn played(test: &str, scenario: &'static str) -> (Events, TurnId) {
    let dir = Dir::new(test, &[(CODEX, Scenario::Local(scenario))]);
    let host: Running = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // An agent-started turn would open for held output once the lawyer's turn has closed.
    host.shutdown();
    events.until_end();
    (events, turn)
}

/// The texts of the thread's notices, each a warning of kind `tool_not_started`.
fn notices(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => {
                assert_eq!(notice.level, NoticeLevel::Warning, "{notice:?}");
                assert_eq!(notice.kind, Some(NoticeKind::ToolNotStarted), "{notice:?}");
                Some(notice.text.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_codex_mcp_server_that_did_not_start_is_no_step() {
    let (events, turn) = played(
        "a_codex_mcp_server_that_did_not_start_is_no_step",
        "codex-mcp-startup-failed",
    );
    // The scenario has no other tool call, so no step and no step's update at all.
    let steps = events.count(|event| {
        matches!(
            event,
            DomainEvent::ToolCallStarted(_) | DomainEvent::ToolCallUpdated(_)
        )
    });
    assert_eq!(steps, 0, "{}", events.dump());
    let turns = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Reading the papers. Done.");
}

/// Three servers, three notices: `silent`'s report and its update make one. The pre-turn one
/// is logged outside the turn, the other two inside it.
#[test]
fn each_server_that_did_not_start_gets_one_notice_in_the_lawyer_s_words() {
    let (events, turn) = played(
        "each_server_that_did_not_start_gets_one_notice",
        "codex-mcp-startup-failed",
    );
    let tail = "didn't start, so this answer runs without it. Ask again to try once more.";
    assert_eq!(
        notices(&events),
        [
            format!("\"fails\" {tail}"),
            format!("\"silent\" {tail}"),
            format!("Case Law {tail}"),
        ],
        "{}",
        events.dump()
    );
    let at = |find: &dyn Fn(&DomainEvent) -> bool| events.position(find).unwrap();
    let started = at(&|e| matches!(e, DomainEvent::TurnStarted(s) if s.turn == turn));
    let first = at(&|e| matches!(e, DomainEvent::Notice(n) if n.text.starts_with("\"fails\"")));
    let case_law = at(&|e| matches!(e, DomainEvent::Notice(n) if n.text.starts_with("Case Law")));
    let ended = at(&|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == turn));
    assert!(first < started, "{}", events.dump());
    assert!(started < case_law && case_law < ended, "{}", events.dump());
}

/// A session whose servers all started tells the lawyer nothing.
#[test]
fn a_server_that_started_gets_no_notice() {
    let (events, turn) = played("a_server_that_started_gets_no_notice", "codex-mcp-started");
    assert_eq!(notices(&events), Vec::<String>::new(), "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Done.");
}
