//! The "key stored" marker file: no file marks no agent; a file that does not parse, or cannot
//! be read, marks every agent; a write over such a file starts from every known agent; the
//! file holds agent ids only; and a read that began before a clear does not mark its agent.

use std::collections::BTreeSet;

use clauseo_agents::keys::Keychain;
use clauseo_protocol::AgentId;
use zeroize::Zeroizing;

use super::{KeyMarker, Marked, path};
use crate::setup::keys::KeyCache;
use crate::test_support::{Scratch, block_on};

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

fn marker(scratch: &Scratch) -> KeyMarker {
    KeyMarker::new(scratch.path(), [&claude(), &codex()])
}

#[test]
fn no_file_marks_no_agent_and_a_set_and_a_clear_write_agent_ids_only() {
    let scratch = Scratch::folder("key-marker-none", &[]);
    let marker = marker(&scratch);
    assert_eq!(marker.marked(), Marked::Only(BTreeSet::new()));
    marker.set(&claude()).unwrap();
    assert_eq!(marker.marked(), Marked::Only([claude()].into()));
    let text = std::fs::read_to_string(path(scratch.path())).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value, serde_json::json!({ "agents": ["claude-acp"] }));
    marker.clear(&claude());
    assert_eq!(marker.marked(), Marked::Only(BTreeSet::new()));
}

#[test]
fn a_corrupt_marker_file_marks_every_agent_and_a_write_over_it_keeps_them_marked() {
    let scratch = Scratch::folder("key-marker-corrupt", &["agents"]);
    scratch.file("agents/key-stored.json", b"{ not json");
    let marker = marker(&scratch);
    assert_eq!(marker.marked(), Marked::All);
    assert!(marker.marked().contains(&codex()));
    // Unmarking Claude over the corrupt file leaves Codex marked: never unmarked by accident.
    marker.clear(&claude());
    assert_eq!(marker.marked(), Marked::Only([codex()].into()));
}

#[test]
fn a_marker_file_with_an_unknown_field_or_a_bad_id_is_corrupt() {
    for (n, text) in [
        &br#"{"agents": ["claude-acp"], "key": "x"}"#[..],
        br#"{"agents": ["Not An Id"]}"#,
    ]
    .into_iter()
    .enumerate()
    {
        let scratch = Scratch::folder(&format!("key-marker-bad-{n}"), &["agents"]);
        scratch.file("agents/key-stored.json", text);
        assert_eq!(marker(&scratch).marked(), Marked::All);
    }
}

#[test]
fn an_unreadable_marker_file_marks_every_agent() {
    let scratch = Scratch::folder("key-marker-unreadable", &[]);
    // A folder where the file goes: it cannot be read as one.
    scratch.dir("agents/key-stored.json");
    assert_eq!(marker(&scratch).marked(), Marked::All);
}

#[test]
fn a_stale_read_that_answers_a_key_after_a_clear_does_not_mark_the_agent() {
    const KEY: &str = "fakeAnthropicKey0123";
    let scratch = Scratch::folder("key-marker-stale", &[]);
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).unwrap();
    let keys = KeyCache::empty(
        keychain.clone(),
        None,
        KeyMarker::new(scratch.path(), [&claude()]),
    );
    keys.marker.set(&claude()).unwrap();
    // A read of Claude's key begins, and finds the key (its answer is not in yet) ...
    let began = keys.marks.begin(&claude()).unwrap();
    // ... then the key is cleared: the key goes, then the marker.
    block_on(keys.clear(&claude(), |_| {})).unwrap();
    // The read answers with the key it found before the clear: the clear wins, so the
    // answer is dropped and nothing is marked or held.
    keys.marks.finish(
        &keys.keys,
        &keys.marker,
        &claude(),
        began,
        Ok(Some(Zeroizing::new(KEY.to_owned()))),
    );
    assert!(!keys.in_memory(&claude()) && !keys.unreadable(&claude()));
    assert_eq!(marker(&scratch).marked(), Marked::Only(BTreeSet::new()));
    assert_eq!(keychain.load(&claude()).unwrap(), None);
}
