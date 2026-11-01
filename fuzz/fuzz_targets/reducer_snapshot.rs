//! reducer: `Snapshot::from_bytes` on raw snapshot bytes, as the host gets them after the store
//! decompresses a snapshot.
//!
//! Items driven: `Snapshot::from_bytes`, `Snapshot::into_state`, `Snapshot::of`,
//! `Snapshot::to_bytes`, `view_snapshot`, `page_entries` and `apply` (the fold of the corpus
//! events onto the decoded state).
//!
//! Input: the snapshot bytes, as they are.
//!
//! The decode runs on a thread with a 2 MiB stack, the default for spawned threads. Plain
//! `rmp_serde::from_slice` (which `from_bytes` uses, with no depth limit set) can overflow a
//! debug build's 2 MiB stack on deep nesting, as the protocol crate's raw.rs warns, so a stack
//! overflow here is a finding.
//!
//! Invariants, on success:
//!   - a snapshot whose schema is not `REDUCER_SCHEMA_VERSION` never decodes as Ok; it is
//!     `Error::SchemaMismatch`, whose `found` differs from `expected`,
//!   - `into_state` is Ok, `to_bytes` is Ok, `from_bytes(to_bytes(s))` gives the same state, and
//!     encoding that state again gives the same bytes ("equal states write equal bytes"),
//!   - `view_snapshot`, `page_entries` and folding the protocol v1 corpus events (renumbered onto
//!     the decoded stream and sequence) do not panic.
#![no_main]

use std::sync::OnceLock;

use clauseo_protocol::{Envelope, PageLimit, Seq, StreamId};
use clauseo_reducer::{
    Error, REDUCER_SCHEMA_VERSION, Snapshot, StreamState, apply, page_entries, view_snapshot,
};
use libfuzzer_sys::fuzz_target;

static EVENTS: &str = include_str!("../../crates/protocol/tests/corpus/v1/events.jsonl");

fn corpus() -> &'static [Envelope] {
    static CELL: OnceLock<Vec<Envelope>> = OnceLock::new();
    CELL.get_or_init(|| {
        EVENTS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("corpus line decodes"))
            .collect()
    })
}

fn exercise(state: &mut StreamState) {
    let _ = view_snapshot(state);
    if let Ok(limit) = PageLimit::new(50) {
        let _ = page_entries(state, None, limit);
        let last = state.entries().keys().next_back().copied();
        let _ = page_entries(state, last, limit);
    }
    let stream = state.stream();
    let matter = matches!(stream, StreamId::Matter(_));
    for event in corpus() {
        if matches!(event.stream, StreamId::Matter(_)) != matter {
            continue;
        }
        let next = state.seq().map_or(1, |s| s.get().saturating_add(1));
        let Ok(seq) = Seq::new(next) else { return };
        let envelope = Envelope::new(
            event.event_id,
            stream,
            seq,
            event.ts,
            event.command_id,
            event.event.clone(),
        );
        let _ = apply(state, &envelope);
    }
}

fn run(data: &[u8]) {
    match Snapshot::from_bytes(data) {
        Ok(snapshot) => {
            assert_eq!(snapshot.reducer_schema, REDUCER_SCHEMA_VERSION);
            let state = snapshot
                .into_state()
                .expect("this schema's snapshot gives its state");
            let bytes = Snapshot::of(&state)
                .to_bytes()
                .unwrap_or_else(|e| panic!("a decoded state does not encode: {e}"));
            let back = Snapshot::from_bytes(&bytes)
                .unwrap_or_else(|e| panic!("re-encoded snapshot does not decode: {e}"))
                .into_state()
                .unwrap();
            assert_eq!(back, state, "snapshot round trip changes the state");
            let again = Snapshot::of(&back).to_bytes().unwrap();
            assert_eq!(again, bytes, "equal states write different bytes");
            let mut state = state;
            exercise(&mut state);
        }
        Err(Error::SchemaMismatch { found, expected }) => {
            assert_eq!(expected, REDUCER_SCHEMA_VERSION);
            assert_ne!(found, REDUCER_SCHEMA_VERSION);
        }
        Err(_) => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let data = data.to_vec();
    let worker = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || run(&data))
        .expect("spawn");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
});
