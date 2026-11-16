//! The per-turn caps: past them a call is refused, and the next turn starts again.

use serde_json::json;

use std::path::PathBuf;

use super::{Fixture, call, ok, read, refused};
use crate::versions::texts::Refusal;
use crate::versions::{MAX_CALLS_PER_TURN, MAX_DOCX_RUNS_PER_TURN, VersionsBackend, lock};

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

/// A stetdoc that is configured but cannot start: each Word read copies its file, spends its run
/// and fails, so no Word text is ever kept.
pub(super) fn missing_stetdoc(fixture: &Fixture) -> PathBuf {
    fixture.dir.path().join("no-such-stetdoc")
}

#[test]
fn word_reads_past_the_turns_cap_are_refused_until_the_next_turn() {
    let mut fixture = Fixture::new("versions-caps-docx");
    fixture.put("letter.docx", b"PK not really");
    let v2 = fixture.checkpoint(2);
    let backend = VersionsBackend::new(fixture.config(Some(missing_stetdoc(&fixture))));
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

#[test]
fn word_reads_racing_for_the_last_runs_take_exactly_the_cap() {
    let mut fixture = Fixture::new("versions-caps-race");
    let extra = 10;
    let files: Vec<String> = (0..MAX_DOCX_RUNS_PER_TURN + extra)
        .map(|n| format!("letter-{n:02}.docx"))
        .collect();
    for (n, file) in files.iter().enumerate() {
        fixture.put(file, format!("PK {n}").as_bytes());
    }
    let v2 = fixture.checkpoint(2);
    let backend = VersionsBackend::new(fixture.config(Some(missing_stetdoc(&fixture))));
    let refusals: Vec<String> = std::thread::scope(|scope| {
        let mut reads = Vec::new();
        for file in &files {
            let backend = backend.clone();
            reads.push(scope.spawn(move || refused(&read(&backend, v2, file)).to_owned()));
        }
        reads.into_iter().map(|read| read.join().unwrap()).collect()
    });
    let failed = refusals
        .iter()
        .filter(|text| *text == Refusal::WordFailed.text())
        .count();
    let capped = refusals
        .iter()
        .filter(|text| *text == Refusal::TooManyDocx.text())
        .count();
    assert_eq!(
        (failed, capped),
        (MAX_DOCX_RUNS_PER_TURN as usize, extra as usize)
    );
    assert_eq!(
        lock(&backend.inner.caps).docx_runs(),
        MAX_DOCX_RUNS_PER_TURN
    );
}

#[test]
fn a_word_read_that_runs_no_stetdoc_gives_its_run_back() {
    let mut fixture = Fixture::new("versions-caps-give-back");
    fixture.put("gone.docx", b"PK gone");
    let v2 = fixture.checkpoint(2);
    fixture.remove("gone.docx");
    fixture.checkpoint(3);
    let backend = VersionsBackend::new(fixture.config(Some(missing_stetdoc(&fixture))));
    assert_eq!(
        refused(&read(&backend, v2, "gone.docx")),
        Refusal::Deleted.text()
    );
    assert_eq!(lock(&backend.inner.caps).docx_runs(), 0);
    // With no stetdoc at all, nothing is reserved and nothing is copied.
    let no_tool = fixture.backend();
    fixture.put("letter.docx", b"PK letter");
    let v4 = fixture.checkpoint(4);
    assert_eq!(
        refused(&read(&no_tool, v4, "letter.docx")),
        Refusal::NoWordTool.text()
    );
    assert_eq!(lock(&no_tool.inner.caps).docx_runs(), 0);
    assert!(!fixture.data().join("tmp").exists());
}

#[test]
fn a_word_file_past_the_size_cap_is_not_copied_and_runs_no_stetdoc() {
    let mut fixture = Fixture::new("versions-caps-capped");
    fixture.put("big.docx", &[b'P'; 100]);
    let v2 = fixture.checkpoint(2);
    *fixture.source.cap.lock().unwrap() = Some(10);
    let backend = VersionsBackend::new(fixture.config(Some(missing_stetdoc(&fixture))));
    assert_eq!(
        refused(&read(&backend, v2, "big.docx")),
        Refusal::WordFailed.text()
    );
    assert_eq!(lock(&backend.inner.caps).docx_runs(), 0);
    let tmp = fixture.data().join("tmp");
    let left: Vec<_> = std::fs::read_dir(&tmp)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}
