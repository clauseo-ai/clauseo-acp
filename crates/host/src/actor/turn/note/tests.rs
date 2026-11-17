//! The note's read of the matter's restores against a real store: placed by the last counting
//! turn's before checkpoint when its after checkpoint has no row, whatever the clock said, or
//! from the conversation's creation with neither; never the whole log's restores.

use clauseo_protocol::{
    CheckpointId, DomainEvent, Envelope, EventId, MatterPath, RestoreScope, Seq, StreamId,
    VersionRestored,
};
use clauseo_store::{CheckpointRow, Commit, fixtures};
use futures::executor::LocalPool;

use super::{Bound, restores_log};
use crate::pipeline::tests::support::{TempDir, id, notice, open_store, ts};

/// A restore of `brief.txt` that wrote checkpoint `n`.
fn restore(n: u64) -> DomainEvent {
    DomainEvent::from(VersionRestored {
        from: id::<CheckpointId>(1_000),
        checkpoint: id::<CheckpointId>(n),
        scope: RestoreScope::Matter,
        undoes: None,
        written: vec![MatterPath::new("brief.txt").unwrap()],
        removed: Vec::new(),
        deleted: Vec::new(),
        more: None,
    })
}

fn envelope(seq: u64, at: i64, event: DomainEvent) -> Envelope {
    Envelope::new(
        id::<EventId>(seq),
        StreamId::Matter(fixtures::matter()),
        Seq::new(seq).unwrap(),
        ts(at),
        None,
        event,
    )
}

/// A failed turn whose before checkpoint's row is at seq 2, and whose after checkpoint the store
/// has no row of: seq 1 is a restore before it; seq 3, after it, was stamped while the clock
/// stood earlier than the turn's end.
#[test]
fn a_failed_turn_s_before_checkpoint_places_the_restores_whatever_the_clock() {
    let dir = TempDir::new("note_restores_before");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let stream = StreamId::Matter(fixtures::matter());
    let before = id::<CheckpointId>(800);
    let commit = Commit {
        events: vec![
            envelope(1, 10, restore(1)),
            envelope(2, 20, notice(1)),
            envelope(3, 5, restore(3)),
        ],
        matter: Some(fixtures::matter_row()),
        checkpoints: vec![CheckpointRow {
            id: before,
            matter: fixtures::matter(),
            commit_oid: "0".repeat(40),
            seq: Seq::new(2).unwrap(),
        }],
        ..Commit::default()
    };
    pool.run_until(store.commit(commit)).unwrap();
    let since = Bound {
        after: Some(id::<CheckpointId>(900)),
        before: Some(before),
        at: ts(1_000),
    };
    let log = pool.run_until(restores_log(&store, stream, since)).unwrap();
    let seqs: Vec<u64> = log.iter().map(|e| e.seq.get()).collect();
    assert_eq!(seqs, [3]);
}

#[test]
fn with_no_checkpoint_row_restores_are_read_from_the_conversation_s_creation() {
    let dir = TempDir::new("note_restores_log");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let stream = StreamId::Matter(fixtures::matter());
    // Seq 1 is an old restore; 600 other events; then a restore after the conversation began.
    let mut events = vec![envelope(1, 10, restore(1))];
    for n in 2..=601 {
        events.push(envelope(n, 10 + i64::try_from(n).unwrap(), notice(1)));
    }
    events.push(envelope(602, 1_000, restore(2)));
    let commit = Commit {
        events,
        matter: Some(fixtures::matter_row()),
        ..Commit::default()
    };
    pool.run_until(store.commit(commit)).unwrap();
    let seqs = |log: Vec<Envelope>| -> Vec<u64> { log.iter().map(|e| e.seq.get()).collect() };

    // Neither checkpoint has a row in the store: from the conversation's creation.
    let unknown = Bound {
        after: Some(id::<CheckpointId>(900)),
        before: Some(id::<CheckpointId>(901)),
        at: ts(900),
    };
    let log = pool
        .run_until(restores_log(&store, stream, unknown))
        .unwrap();
    assert_eq!(seqs(log), [602]);

    // No turn carried the note: from the conversation's creation, here before both.
    let created = Bound {
        after: None,
        before: None,
        at: ts(5),
    };
    let log = pool
        .run_until(restores_log(&store, stream, created))
        .unwrap();
    assert_eq!(seqs(log), [1, 602]);
}
