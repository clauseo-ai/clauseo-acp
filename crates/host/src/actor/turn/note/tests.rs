//! The note's read of the matter's restores against a real store: from a turn's checkpoint the
//! store has no row of, or from the conversation's creation, only the restores logged since are
//! read, never the whole log's.

use clauseo_protocol::{
    CheckpointId, DomainEvent, Envelope, EventId, MatterPath, RestoreScope, Seq, StreamId,
    VersionRestored,
};
use clauseo_store::{Commit, fixtures};
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

#[test]
fn restores_are_read_only_from_the_turn_s_end_or_the_conversation_s_creation() {
    let dir = TempDir::new("note_restores_log");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let stream = StreamId::Matter(fixtures::matter());
    // Seq 1 is an old restore; 600 other events; then a restore after the turn ended.
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

    // The turn's after checkpoint has no row in the store: from the time it ended.
    let ended = Bound {
        checkpoint: Some(id::<CheckpointId>(900)),
        at: ts(900),
    };
    let log = pool.run_until(restores_log(&store, stream, ended)).unwrap();
    assert_eq!(seqs(log), [602]);

    // No turn carried the note: from the conversation's creation, here before both.
    let created = Bound {
        checkpoint: None,
        at: ts(5),
    };
    let log = pool
        .run_until(restores_log(&store, stream, created))
        .unwrap();
    assert_eq!(seqs(log), [1, 602]);
}
