//! The `keychain` row, its Ask and the agents' keys read again after a granted one, over the
//! fake [`Probe`]; and what a silent read's answer means (`codes.rs`).
// unix-paths-ok: the macOS paths are only compared, both sides resolved alike

use std::cell::Cell;
use std::path::PathBuf;

use super::codes::{INTERACTION_NOT_ALLOWED, ITEM_NOT_FOUND, silent_answer};
use super::keychain::keychain_row;
use super::store::Answer;
use super::tests::{Fake, NONE, item, rows, store};
use super::*;

/// A Mac whose matters folder is outside Documents, so only the keychain may be listed.
fn mac() -> Fake {
    let mut probe = Fake::new(Platform::Mac);
    probe.matters = Some(PathBuf::from("/Users/ada/Clauseo"));
    probe
}

#[test]
fn keychain_is_listed_only_when_an_item_would_ask() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut probe = mac();
    assert_eq!(rows(&probe, &store), NONE);

    probe.keychain.would_ask = vec![
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop.mcp", "files"),
    ];
    assert_eq!(
        rows(&probe, &store),
        [PermissionRow {
            id: PermissionId::Keychain,
            state: OsPermissionState::WouldAsk,
            folder: None,
            settings_path: None,
            note: Some("ai.clauseo.desktop (claude-acp), ai.clauseo.desktop.mcp (files)".into()),
        }]
    );

    probe.keychain.would_ask.clear();
    probe.keychain.unknown = vec![item("ai.clauseo.desktop.connectors", "caselaw")];
    assert_eq!(rows(&probe, &store)[0].state, OsPermissionState::Unknown);
}

#[test]
fn asking_the_keychain_reads_each_item_and_names_the_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain.would_ask = vec![
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop", "codex-acp"),
    ];
    probe.keychain.unknown = vec![item("ai.clauseo.desktop.mcp", "files")];
    probe.refused = vec!["codex-acp"];
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &|| {}).unwrap();
    assert_eq!(probe.asked(), ["claude-acp", "codex-acp", "files"]);
    assert_eq!(row.state, OsPermissionState::Denied);
    assert_eq!(row.note.as_deref(), Some("ai.clauseo.desktop (codex-acp)"));
    assert_eq!(
        rows(&probe, &store(&dir, "1"))[0].state,
        OsPermissionState::Denied
    );

    probe.refused.clear();
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &|| {}).unwrap();
    assert_eq!((row.state, row.note), (OsPermissionState::Granted, None));
    // Granted on this build: still shown once nothing would ask, so it is not asked again.
    probe.keychain = KeychainCheck::default();
    assert_eq!(
        rows(&probe, &store(&dir, "1")),
        [PermissionRow {
            id: PermissionId::Keychain,
            state: OsPermissionState::Granted,
            folder: None,
            settings_path: None,
            note: None,
        }]
    );
    assert_eq!(rows(&probe, &store(&dir, "2")), NONE);
}

#[test]
fn a_granted_keychain_ask_reads_the_keys_again_once_and_a_denied_one_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain.would_ask = vec![
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop.connectors", "caselaw"),
    ];
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);

    probe.refused = vec!["caselaw"];
    ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    assert_eq!(reads.get(), 0, "denied: nothing read again");

    probe.refused.clear();
    ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    assert_eq!(reads.get(), 1, "granted: read again once");
}

#[test]
fn asking_for_documents_never_reads_the_keys_again() {
    let dir = tempfile::tempdir().unwrap();
    let probe = Fake::new(Platform::Mac);
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(
        &probe,
        &store(&dir, "1"),
        PermissionId::FilesDocuments,
        &count,
    )
    .unwrap();
    assert_eq!(row.state, OsPermissionState::Granted);
    assert_eq!(reads.get(), 0);
}

#[test]
fn the_app_s_hook_reads_nothing_before_the_keys_load() {
    // The setup thread has not filled the slot: its own load reads every key, so nothing to do.
    KeychainGranted::reading(crate::setup::KeySlot::default()).run();
}

#[test]
fn a_silent_read_that_needs_a_dialog_would_ask() {
    // Another build's access list, with dialogs off.
    assert_eq!(silent_answer(Err(INTERACTION_NOT_ALLOWED)), Some(false));
}

#[test]
fn a_locked_keychain_would_ask_too() {
    // A locked login keychain answers the same code with dialogs off; the Ask covers it (macOS
    // then asks to unlock), so it reads as one that would ask, whatever the cause.
    assert_eq!(silent_answer(Err(-25308)), Some(false));
}

#[test]
fn a_silent_read_that_reads_needs_no_asking() {
    assert_eq!(silent_answer(Ok(true)), Some(true));
}

#[test]
fn an_item_gone_since_it_was_listed_needs_no_asking() {
    // As the Ask's read counts it: nothing left to ask for.
    assert_eq!(silent_answer(Err(ITEM_NOT_FOUND)), Some(true));
}

#[test]
fn any_other_answer_cannot_be_told() {
    // errSecAuthFailed, errSecNotAvailable, and an answer with no value.
    for read in [Err(-25293), Err(-25291), Ok(false)] {
        assert_eq!(silent_answer(read), None, "{read:?}");
    }
}

/// The keychain row's state for `check`, with `recorded` the answer on this build.
fn keychain_state(check: &KeychainCheck, recorded: Option<Answer>) -> Option<OsPermissionState> {
    keychain_row(check, recorded).map(|row| row.state)
}

/// A check whose items `would_ask` and `unknown` name.
fn check(would_ask: &[&str], unknown: &[&str]) -> KeychainCheck {
    let items = |accounts: &[&str]| {
        accounts
            .iter()
            .map(|account| item("ai.clauseo.desktop", account))
            .collect()
    };
    KeychainCheck {
        would_ask: items(would_ask),
        unknown: items(unknown),
    }
}

#[test]
fn an_item_that_would_ask_shows_would_ask_even_after_a_grant() {
    let found = check(&["claude-acp"], &[]);
    assert_eq!(
        keychain_state(&found, None),
        Some(OsPermissionState::WouldAsk)
    );
    // An "Allow" once: the grant was recorded, yet a read asks again; the fact wins.
    assert_eq!(
        keychain_state(&found, Some(Answer::Granted)),
        Some(OsPermissionState::WouldAsk)
    );
}

#[test]
fn a_refusal_stays_denied_while_a_read_would_still_ask() {
    let found = check(&["claude-acp"], &["files"]);
    assert_eq!(
        keychain_state(&found, Some(Answer::Denied)),
        Some(OsPermissionState::Denied)
    );
}

#[test]
fn nothing_that_would_ask_shows_granted_once_asked_and_no_row_otherwise() {
    let none = KeychainCheck::default();
    assert_eq!(keychain_state(&none, None), None);
    assert_eq!(
        keychain_state(&none, Some(Answer::Granted)),
        Some(OsPermissionState::Granted)
    );
    // Refused then, but every item reads now (allowed since, or gone).
    assert_eq!(
        keychain_state(&none, Some(Answer::Denied)),
        Some(OsPermissionState::Granted)
    );
}

#[test]
fn an_item_that_cannot_be_told_shows_the_recorded_answer_else_unknown() {
    let found = check(&[], &["files"]);
    assert_eq!(
        keychain_state(&found, None),
        Some(OsPermissionState::Unknown)
    );
    assert_eq!(
        keychain_state(&found, Some(Answer::Granted)),
        Some(OsPermissionState::Granted)
    );
    assert_eq!(
        keychain_state(&found, Some(Answer::Denied)),
        Some(OsPermissionState::Denied)
    );
}

#[test]
fn a_later_listing_on_the_same_build_follows_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain.would_ask = vec![item("ai.clauseo.desktop", "claude-acp")];
    ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &|| {}).unwrap();
    // Granted, but the next start's check finds the item asking again.
    assert_eq!(
        rows(&probe, &store(&dir, "1"))[0].state,
        OsPermissionState::WouldAsk
    );
}
