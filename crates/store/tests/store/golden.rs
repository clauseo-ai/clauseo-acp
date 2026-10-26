//! The stored envelope's bytes, pinned: the on-disk form outlives any release.
//!
//! `stored_envelope_matches_golden` commits three envelopes (a `TextAppended` with no
//! `command_id`, a `ToolCallStarted` with its optional fields `None`, and an envelope of the
//! kind `future_kind`, which this version does not know), closes the store, reads each one's
//! `events.envelope` column with `rusqlite`, and compares it with
//! `tests/golden/<kind>.msgpack`.
//!
//! The files were made by this test: `CLAUSEO_BLESS=1 cargo nextest run -p clauseo-store
//! stored_envelope_matches_golden` writes the stored bytes to them (through `write.rs`) instead
//! of comparing. Bless only on purpose: a changed file is a change to the on-disk format, which
//! needs a migration or an upcaster, never a silent rewrite.

use std::path::PathBuf;

use clauseo_protocol::{
    CompactString, DomainEvent, Envelope, EventId, ToolCallStarted, ToolCallStatus, ToolKind,
    TurnId,
};
use clauseo_store::Commit;
use futures::executor::block_on;

use crate::common::{
    TestDir, id, seq, sql, stored_envelope, text_event, thread, ts, unknown_event,
};
use crate::write;

/// A `ToolCallStarted` at `n` with `tool_name`, `raw_input` and `raw_output` `None`.
fn tool_call_started(n: u64) -> Envelope {
    let event = DomainEvent::from(Box::new(ToolCallStarted {
        turn: id::<TurnId>(500),
        tool_call: id(600),
        acp_id: CompactString::new("toolu_01"),
        title: CompactString::new("Read the lease"),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: vec![],
        raw_input: None,
        raw_output: None,
        content: vec![],
    }));
    Envelope::new(
        id::<EventId>(1_000_000 + n),
        thread(1),
        seq(n),
        ts(n.cast_signed()),
        None,
        event,
    )
}

fn golden(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{kind}.msgpack"))
}

#[test]
fn stored_envelope_matches_golden() {
    let dir = TestDir::new("stored_envelope_matches_golden");
    let events = vec![
        text_event(
            thread(1),
            1,
            "The tenant shall keep the premises in good repair.",
        ),
        tool_call_started(2),
        unknown_event(thread(1), 3),
    ];
    assert!(events[0].command_id.is_none());
    let store = dir.open();
    block_on(store.commit(Commit {
        events: events.clone(),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();

    let bless = std::env::var_os("CLAUSEO_BLESS").is_some_and(|value| value == "1");
    let conn = sql(&dir);
    let mut differ = Vec::new();
    for (n, envelope) in (1..).zip(&events) {
        let kind = envelope.event.kind();
        let stored = stored_envelope(&conn, thread(1), n);
        let path = golden(kind);
        if bless {
            write::create_dir_all(path.parent().unwrap());
            write::write(&path, &stored);
            continue;
        }
        let expected = std::fs::read(&path).unwrap_or_else(|error| {
            panic!("{}: {error}; bless with CLAUSEO_BLESS=1", path.display())
        });
        if stored != expected {
            differ.push(kind.to_owned());
        }
    }
    assert!(
        differ.is_empty(),
        "the stored envelope of {differ:?} differs from tests/golden/"
    );
}
