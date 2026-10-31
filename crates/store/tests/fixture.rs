//! The 20k-event reopen fixture keeps the shape its doc comments promise, so the store's reopen
//! bench and host's whole-reopen bench time the thread they say they time: 20,000 events in
//! 80 turns of 250, commits cut at turn ends and at the snapshot, and the events of each turn
//! by kind, size and order. Built in memory; nothing here touches a disk.

#![cfg(test)]

use std::collections::{BTreeMap, HashSet};

use clauseo_protocol::{
    ContentBlock, DomainEvent, Envelope, Seq, ToolCallId, ToolCallStatus, TurnId,
};
use clauseo_store::SnapshotIn;
use clauseo_store::fixtures::{
    EVENTS, EVENTS_PER_TURN, SNAPSHOT_SEQ, TEXT_BYTES, TURNS, matter_row, reopen_20k, stream,
    thread_row,
};

/// The reducer schema the test passes; any number works.
const SCHEMA: u32 = 7;

/// The fixture's commits, and the events its `snapshot` callback was given.
fn build() -> (Vec<clauseo_store::Commit>, Vec<u64>) {
    let mut given = Vec::new();
    let commits = reopen_20k(
        |events: &[Envelope]| {
            given = events.iter().map(|envelope| envelope.seq.get()).collect();
            b"reduced state".to_vec()
        },
        SCHEMA,
    );
    (commits, given)
}

/// The turn an event belongs to, for the kinds that name one.
fn turn_of(event: &DomainEvent) -> Option<TurnId> {
    match event {
        DomainEvent::UserMessage(event) => Some(event.turn),
        DomainEvent::TurnStarted(event) => Some(event.turn),
        DomainEvent::TextAppended(event) => Some(event.turn),
        DomainEvent::ToolCallStarted(event) => Some(event.turn),
        DomainEvent::FilesChanged(event) => Some(event.turn),
        DomainEvent::TurnEnded(event) => Some(event.turn),
        _ => None,
    }
}

/// The commits run seq 1 to 20,000 in order, one per turn, with the turn holding the snapshot
/// cut after it: the first carries the thread's matter and thread rows, and only the commit
/// ending at `SNAPSHOT_SEQ` carries the snapshot, of events 1 to `SNAPSHOT_SEQ`.
#[test]
fn reopen_fixture_commits_cut_at_turn_ends_and_the_snapshot() {
    let (commits, given) = build();

    let seqs: Vec<u64> = commits
        .iter()
        .flat_map(|commit| commit.events.iter().map(|envelope| envelope.seq.get()))
        .collect();
    assert_eq!(seqs, (1..=EVENTS).collect::<Vec<_>>());

    let mut ends: Vec<u64> = (1..=TURNS).map(|turn| turn * EVENTS_PER_TURN).collect();
    ends.push(SNAPSHOT_SEQ);
    ends.sort_unstable();
    let found: Vec<u64> = commits
        .iter()
        .map(|commit| commit.events.last().unwrap().seq.get())
        .collect();
    assert_eq!(found, ends);

    for (index, commit) in commits.iter().enumerate() {
        let first = index == 0;
        assert_eq!(
            commit.matter,
            first.then(matter_row),
            "commit {index}'s matter row"
        );
        assert_eq!(
            commit.thread,
            first.then(thread_row),
            "commit {index}'s thread row"
        );
        let at_snapshot = commit.events.last().unwrap().seq.get() == SNAPSHOT_SEQ;
        let expected = at_snapshot.then(|| {
            SnapshotIn::compress(
                stream(),
                Seq::new(SNAPSHOT_SEQ).unwrap(),
                SCHEMA,
                b"reduced state",
            )
        });
        assert_eq!(commit.snapshot, expected, "commit {index}'s snapshot");
    }
    assert_eq!(given, (1..=SNAPSHOT_SEQ).collect::<Vec<_>>());
}

/// Every turn of 250 events is one turn: its events name one turn id of their own, it opens
/// with a `UserMessage` and a `TurnStarted`, closes with a `FilesChanged` and a `TurnEnded`,
/// and holds 196 `TextAppended` (195 and the `ThreadCreated` at seq 1 in the first) and 25 tool
/// calls, each `ToolCallStarted` followed at once by the `ToolCallUpdated` that completes it.
/// Every text is `TEXT_BYTES` long and differs from the others, every tool call has its own id,
/// and times rise with `seq` up to the thread row's `updated`.
#[test]
fn reopen_fixture_turns_have_the_promised_shape() {
    let (commits, _) = build();
    let events: Vec<Envelope> = commits
        .into_iter()
        .flat_map(|commit| commit.events)
        .collect();
    assert!(events.iter().all(|envelope| envelope.stream == stream()));

    let mut turns = HashSet::new();
    for (index, turn) in events
        .chunks(usize::try_from(EVENTS_PER_TURN).unwrap())
        .enumerate()
    {
        let ids: HashSet<TurnId> = turn.iter().filter_map(|e| turn_of(&e.event)).collect();
        assert_eq!(ids.len(), 1, "turn {index} names one turn id");
        assert!(
            turns.insert(*ids.iter().next().unwrap()),
            "turn {index} has its own id"
        );

        let kinds: Vec<&str> = turn.iter().map(|envelope| envelope.event.kind()).collect();
        let opening = if index == 0 {
            &kinds[1..3]
        } else {
            &kinds[..2]
        };
        assert_eq!(
            opening,
            ["user_message", "turn_started"],
            "turn {index} opens"
        );
        assert_eq!(
            kinds[248..],
            ["files_changed", "turn_ended"],
            "turn {index} closes"
        );
        let mut counts = BTreeMap::new();
        for kind in &kinds {
            *counts.entry(*kind).or_insert(0) += 1;
        }
        let mut expected = BTreeMap::from([
            ("user_message", 1),
            ("turn_started", 1),
            ("text_appended", 196),
            ("tool_call_started", 25),
            ("tool_call_updated", 25),
            ("files_changed", 1),
            ("turn_ended", 1),
        ]);
        if index == 0 {
            expected.insert("text_appended", 195);
            expected.insert("thread_created", 1);
        }
        assert_eq!(counts, expected, "turn {index}'s events by kind");
    }
    assert_eq!(turns.len(), usize::try_from(TURNS).unwrap());

    let mut calls: HashSet<ToolCallId> = HashSet::new();
    for pair in events.windows(2) {
        if let DomainEvent::ToolCallStarted(started) = &pair[0].event {
            let DomainEvent::ToolCallUpdated(updated) = &pair[1].event else {
                panic!("seq {} is not followed by its update", pair[0].seq.get());
            };
            assert_eq!(updated.tool_call, started.tool_call);
            assert_eq!(updated.patch.status, Some(ToolCallStatus::Completed));
            assert!(
                calls.insert(started.tool_call),
                "tool call {} repeats",
                started.tool_call
            );
        }
    }
    assert_eq!(calls.len(), 25 * usize::try_from(TURNS).unwrap());

    let texts: Vec<&str> = events
        .iter()
        .filter_map(|envelope| match &envelope.event {
            DomainEvent::TextAppended(event) => Some(event.text.as_str()),
            DomainEvent::UserMessage(event) => match event.content.as_slice() {
                [ContentBlock::Text { text }] => Some(text.as_str()),
                other => panic!("a user message of {other:?}"),
            },
            _ => None,
        })
        .collect();
    assert!(texts.iter().all(|text| text.len() == TEXT_BYTES));
    assert_eq!(texts.iter().collect::<HashSet<_>>().len(), texts.len());

    assert!(events.windows(2).all(|pair| pair[0].ts < pair[1].ts));
    let row = thread_row();
    assert!(row.created <= events[0].ts);
    assert_eq!(row.updated, events.last().unwrap().ts);
}
