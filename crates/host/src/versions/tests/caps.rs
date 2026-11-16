//! The per-turn caps: past them a call is refused, and the next turn starts again.

use serde_json::json;

use super::{Fixture, call, ok, read, refused};
use crate::versions::texts::Refusal;
use crate::versions::{MAX_CALLS_PER_TURN, MAX_DOCX_RUNS_PER_TURN};

#[test]
fn calls_past_the_turns_cap_are_refused_until_the_next_turn() {
    let fixture = Fixture::new("versions-caps-calls");
    let backend = fixture.backend();
    let turns = backend.turns();
    turns.start();
    for _ in 0..MAX_CALLS_PER_TURN {
        ok(&call(&backend, "list_versions", json!({ "limit": 1 })));
    }
    let over = call(&backend, "list_versions", json!({ "limit": 1 }));
    assert_eq!(refused(&over), Refusal::TooManyCalls.text());
    let read_over = call(&backend, "read_version", json!({}));
    assert_eq!(refused(&read_over), Refusal::TooManyCalls.text());
    // The turn ends: what follows counts against a fresh bucket.
    turns.end();
    ok(&call(&backend, "list_versions", json!({ "limit": 1 })));
    turns.start();
    for _ in 0..MAX_CALLS_PER_TURN {
        ok(&call(&backend, "list_versions", json!({ "limit": 1 })));
    }
    refused(&call(&backend, "list_versions", json!({ "limit": 1 })));
}

#[test]
fn word_reads_past_the_turns_cap_are_refused_until_the_next_turn() {
    let mut fixture = Fixture::new("versions-caps-docx");
    // Not a Word file inside: with no stetdoc each read fails after its copy, which counts.
    fixture.put("letter.docx", b"PK not really");
    let v2 = fixture.checkpoint(2);
    let backend = fixture.backend();
    let turns = backend.turns();
    turns.start();
    for _ in 0..MAX_DOCX_RUNS_PER_TURN {
        assert_eq!(
            refused(&read(&backend, v2, "letter.docx")),
            Refusal::WordFailed.text()
        );
    }
    assert_eq!(
        refused(&read(&backend, v2, "letter.docx")),
        Refusal::TooManyDocx.text()
    );
    turns.start();
    assert_eq!(
        refused(&read(&backend, v2, "letter.docx")),
        Refusal::WordFailed.text()
    );
    // Every copy went with its read.
    let tmp = fixture.data().join("tmp");
    let left: Vec<_> = std::fs::read_dir(&tmp)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}
