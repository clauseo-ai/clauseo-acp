//! The turn: event order, the two text stages, checkpoints and `FilesChanged`, agent exit.

use std::time::Duration;

use clauseo_protocol::{
    ChangeKind, CheckpointKind, DetachReason, DomainEvent, ErrorCode, FileChange, StopReason,
    StreamId, TextKind,
};

use crate::observe::{Events, ended_with};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// The display batch: the view gets coalesced text this long after the first delta.
const DISPLAY: Duration = Duration::from_millis(16);
/// The durable batch: `TextAppended` is committed this long after the first un-durable delta.
const DURABLE: Duration = Duration::from_millis(250);

/// A label for a file change's kind.
pub(crate) fn change_label(change: &FileChange) -> &'static str {
    match change.change() {
        ChangeKind::Added => "added",
        ChangeKind::Modified => "modified",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Renamed { .. } => "renamed",
        ChangeKind::Unknown { .. } => "unknown",
    }
}

/// Every `TextAppended` as (kind, text), in order.
fn texts(events: &Events) -> Vec<(TextKind, String)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::TextAppended(appended) => Some((appended.kind, appended.text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn prompt_turn_event_order() {
    let dir = Dir::new(
        "prompt_turn_event_order",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise the indemnity clause.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();

    assert_eq!(
        events.kinds(),
        [
            "thread_created",
            "user_message",
            "agent_session_attached",
            "turn_started",
            "text_appended",
            "text_appended",
            "turn_ended",
        ],
        "{}",
        events.dump()
    );
    assert_eq!(ended.stop, StopReason::EndTurn);
    assert_eq!(
        texts(&events),
        [
            (
                TextKind::Thought,
                "Reading the indemnity clause first.".to_owned()
            ),
            (
                TextKind::Answer,
                "The clause caps liability at the fees paid in the last twelve months.".to_owned()
            ),
        ]
    );

    let mut matter_events = host.events(StreamId::Matter(matter));
    matter_events.until_caught_up();
    let import = matter_events
        .checkpoint(CheckpointKind::Import)
        .expect("the matter's import checkpoint");
    let before = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnStarted(started) => Some(started.checkpoint_before),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        before, import,
        "an unchanged folder reuses the import checkpoint"
    );
    host.shutdown();
}

/// Uses `cancel-unanswered` rather than `prompt-stream`: its one answer chunk is followed by a
/// wait for `session/cancel`, so the prompt cannot end (and flush the text) while the test is
/// still looking at the 16 ms mark. `prompt-stream` answers at once and has two messages, the
/// first of which goes durable when the second one starts.
#[test]
fn text_reaches_view_before_durable() {
    let dir = Dir::new(
        "text_reaches_view_before_durable",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let mut view = host.view(stream, None);
    view.until_items(1);
    let mut events = host.events(stream);
    events.until_caught_up();
    let is_text = |event: &DomainEvent| matches!(event, DomainEvent::TextAppended(_));

    host.prompt(thread, "Summarise the cap.");
    // The first delta arms the display timer, then the durable one, on the host thread. Moving
    // the clock between the two would arm the durable timer 16 ms late, so wait for both.
    host.wait_armed(DISPLAY);
    host.wait_armed(DURABLE);
    host.clock.advance(DISPLAY);
    view.until("the answer text in the view", |v| v.mentions("The clause "));
    host.settle();
    events.drain_ready();
    assert_eq!(
        events.count(is_text),
        0,
        "durable too early: {}",
        events.dump()
    );

    host.clock.advance(DURABLE.checked_sub(DISPLAY).unwrap());
    events.until("a TextAppended", |e| e.count(is_text) >= 1);
    host.settle();
    events.drain_ready();
    assert_eq!(events.count(is_text), 1, "{}", events.dump());
    assert_eq!(events.text(TextKind::Answer), "The clause ");
    events.assert_no_mismatch();
    host.shutdown_past_cancel();
}

#[test]
fn files_one_turn_checkpoints_and_files_changed() {
    let dir = Dir::new(
        "files_one_turn_checkpoints_and_files_changed",
        &[(FAKE, Scenario::Bundled("files-one-turn"))],
    );
    let host = dir.start(0);
    let seed = [
        ("brief.txt", "Clause 4 draft\n"),
        ("old.txt", "Old terms\n"),
    ];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &seed, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Tidy the folder.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);

    let changed = events
        .events()
        .find_map(|event| match event {
            DomainEvent::FilesChanged(changed) if changed.turn == turn => Some(changed.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let mut files: Vec<(&str, &str, bool)> = changed
        .files
        .iter()
        .map(|f| {
            (
                f.path().as_str(),
                change_label(f),
                f.may_include_lawyer_edits(),
            )
        })
        .collect();
    files.sort_unstable();
    assert_eq!(
        files,
        [
            ("brief.txt", "modified", false),
            ("notes/a.txt", "added", false),
            ("old.txt", "deleted", false),
        ]
    );

    let mut matter_events = host.events(StreamId::Matter(matter));
    matter_events.until_caught_up();
    let after = matter_events.checkpoint(CheckpointKind::AfterTurn);
    assert!(
        after.is_some(),
        "no AfterTurn checkpoint: {}",
        matter_events.dump()
    );
    assert_eq!(ended.checkpoint_after, after);

    let read = |path: &str| std::fs::read_to_string(folder.join(path)).ok();
    assert_eq!(read("notes/a.txt").as_deref(), Some("Alpha\n"));
    assert_eq!(read("brief.txt").as_deref(), Some("Clause 4 final\n"));
    assert_eq!(read("old.txt"), None);
    host.shutdown();
}

#[test]
fn agent_exit_fails_turn() {
    let dir = Dir::new(
        "agent_exit_fails_turn",
        &[(FAKE, Scenario::Bundled("exit-mid-turn"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Start the answer.");
    let is_exit = |event: &DomainEvent| {
        matches!(event, DomainEvent::AgentSessionDetached(d)
            if matches!(d.reason, DetachReason::Exited { .. }))
    };
    events.until("AgentSessionDetached { Exited }", |e| {
        e.position(is_exit).is_some()
    });

    let failed = events.position(|event| {
        matches!(event, DomainEvent::TurnFailed(f)
            if f.turn == turn && f.code == ErrorCode::AgentExited)
    });
    let ended = events.position(|event| ended_with(event, turn, StopReason::Failed));
    let exited = events.position(is_exit);
    let (Some(failed), Some(ended), Some(exited)) = (failed, ended, exited) else {
        panic!(
            "want TurnFailed(agent.exited), TurnEnded(Failed), Exited: {}",
            events.dump()
        );
    };
    assert!(failed < ended && ended < exited, "{}", events.dump());
    let Some(DomainEvent::AgentSessionDetached(detached)) =
        events.seen.get(exited).map(|e| &e.event)
    else {
        unreachable!()
    };
    let DetachReason::Exited { code, stderr_tail } = &detached.reason else {
        unreachable!()
    };
    assert_eq!(*code, Some(1));
    assert!(
        stderr_tail.len() <= 4096,
        "stderr tail is {} bytes",
        stderr_tail.len()
    );
    host.shutdown();
}
