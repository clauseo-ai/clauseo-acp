//! The `keychain` row, its Ask through the child and the agents' keys read again after a
//! granted one, over the fake [`Probe`]; and what a silent read's answer means (`codes.rs`).
// unix-paths-ok: the macOS paths are only compared, both sides resolved alike

use std::cell::Cell;
use std::path::PathBuf;

use super::ask_child::{AskItem, AskResult, Outcome};
use super::codes::silent_answer;
use super::codes::{AUTH_FAILED, IN_DARK_WAKE, INTERACTION_NOT_ALLOWED, ITEM_NOT_FOUND};
use super::keychain::{Decision, decide, keychain_state, list_taking_late_grant};
use super::store::Answer;
use super::tests::{Fake, NONE, item, rows, store};
use super::*;

/// A Mac whose matters folder is outside Documents, so only the keychain may be listed.
fn mac() -> Fake {
    let mut probe = Fake::new(Platform::Mac);
    probe.matters = Some(PathBuf::from("/Users/ada/Clauseo"));
    probe
}

/// A check whose `ai.clauseo.desktop` items `would_ask` and `unknown` name.
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

/// The child's answer for `account`.
fn answer(account: &str, read: Result<bool, i32>) -> AskResult {
    let item = AskItem {
        service: "ai.clauseo.desktop".to_owned(),
        account: account.to_owned(),
    };
    AskResult::of(item, read)
}

#[test]
fn keychain_is_listed_only_when_an_item_would_ask_or_cannot_be_told() {
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
            asked: false,
        }]
    );

    probe.keychain.would_ask.clear();
    probe.keychain.unknown = vec![item("ai.clauseo.desktop.connectors", "caselaw")];
    assert_eq!(rows(&probe, &store)[0].state, OsPermissionState::Unknown);
}

#[test]
fn always_allow_reads_everything_after_the_child_records_granted_and_reads_the_keys_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &["files"]);
    probe.after_ask = Some(KeychainCheck::default());
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    // Every would-ask and unknown item went to the child, one by one.
    assert_eq!(probe.asked(), ["keychain:", "claude-acp", "files"]);
    assert_eq!(
        (row.state, row.asked, row.note),
        (OsPermissionState::Granted, true, None)
    );
    assert_eq!(reads.get(), 1, "read again once");
    let shown = rows(&probe, &store(&dir, "1"));
    assert_eq!(
        (shown[0].state, shown[0].asked),
        (OsPermissionState::Granted, true)
    );
    // Another build asked nothing.
    assert_eq!(rows(&probe, &store(&dir, "2")), NONE);
}

#[test]
fn allow_once_answers_would_ask_marked_asked_and_reads_nothing_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &[]);
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    assert_eq!((row.state, row.asked), (OsPermissionState::WouldAsk, true));
    assert_eq!(row.note.as_deref(), Some("ai.clauseo.desktop (claude-acp)"));
    assert_eq!(reads.get(), 0);
    let shown = rows(&probe, &store(&dir, "1"));
    assert_eq!(
        (shown[0].state, shown[0].asked),
        (OsPermissionState::WouldAsk, true)
    );
}

#[test]
fn deny_records_denied_and_reads_nothing_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp", "codex-acp"], &[]);
    probe.after_ask = Some(check(&["codex-acp"], &[]));
    probe.refused = vec!["codex-acp"];
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    assert_eq!((row.state, row.asked), (OsPermissionState::Denied, true));
    assert_eq!(row.note.as_deref(), Some("ai.clauseo.desktop (codex-acp)"));
    assert_eq!(reads.get(), 0);
    assert_eq!(
        rows(&probe, &store(&dir, "1"))[0].state,
        OsPermissionState::Denied
    );
}

#[test]
fn an_always_allow_after_the_ask_is_taken_in_at_the_next_listing_once() {
    // macOS's dialog outlives a killed child (r7715): its Always Allow can come after the Ask.
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir, "1");
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &[]);
    probe.refused = vec!["claude-acp"];
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(&probe, &store, PermissionId::Keychain, &count).unwrap();
    assert_eq!(row.state, OsPermissionState::Denied);
    // Still refused: the listing reads nothing again.
    let shown = list_taking_late_grant(&probe, &store, &count);
    assert_eq!(
        (shown[0].state, reads.get()),
        (OsPermissionState::Denied, 0)
    );

    // Always Allow on the dialog left on screen: nothing would ask now.
    probe.after_ask = Some(KeychainCheck::default());
    let shown = list_taking_late_grant(&probe, &store, &count);
    assert_eq!(
        (shown[0].state, shown[0].asked, reads.get()),
        (OsPermissionState::Granted, true, 1)
    );
    assert_eq!(
        store.asked().get(&PermissionId::Keychain),
        Some(&Answer::Granted)
    );
    list_taking_late_grant(&probe, &store, &count);
    assert_eq!(reads.get(), 1, "read again once");
}

#[test]
fn while_the_child_runs_a_listing_and_a_second_ask_answer_asking_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &[]);
    probe
        .asking
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let shown = rows(&probe, &store(&dir, "1"));
    assert_eq!(
        shown,
        [PermissionRow {
            id: PermissionId::Keychain,
            state: OsPermissionState::Asking,
            folder: None,
            settings_path: None,
            note: None,
            asked: false,
        }]
    );
    let reads = Cell::new(0);
    let count = || reads.set(reads.get() + 1);
    let row = ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &count).unwrap();
    assert_eq!(row.state, OsPermissionState::Asking);
    assert_eq!(probe.asked(), Vec::<String>::new(), "no second child");
    assert_eq!(reads.get(), 0);
}

#[test]
fn the_asking_mark_ends_with_the_ask() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &[]);
    ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &|| {}).unwrap();
    assert!(!probe.asking.load(std::sync::atomic::Ordering::SeqCst));
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
    assert_eq!((row.state, row.asked), (OsPermissionState::Granted, true));
    assert_eq!(reads.get(), 0);
}

#[test]
fn the_app_s_hook_reads_nothing_before_the_keys_load() {
    // The setup thread has not filled the slot: its own load reads every key, so nothing to do.
    KeychainGranted::reading(crate::setup::KeySlot::default()).run();
}

#[test]
fn the_decision_follows_the_check_after_the_child() {
    let read = [answer("claude-acp", Ok(true))];
    assert_eq!(decide(&read, &check(&[], &[])), Decision::Granted);
    // Unknown items left do not hold back a grant; the row then says unknown.
    assert_eq!(decide(&read, &check(&[], &["files"])), Decision::Granted);
    assert_eq!(
        decide(&read, &check(&["claude-acp"], &[])),
        Decision::AllowedOnce
    );
    let refused = [answer("claude-acp", Err(-128))];
    assert_eq!(
        decide(&refused, &check(&["claude-acp"], &[])),
        Decision::Denied
    );
    // An item still asking that the child never answered counts as refused.
    assert_eq!(decide(&read, &check(&["codex-acp"], &[])), Decision::Denied);
    // Refused but readable now (allowed elsewhere since): the fact wins.
    assert_eq!(decide(&refused, &check(&[], &[])), Decision::Granted);
    let gone = [answer("claude-acp", Ok(false))];
    assert_eq!(
        decide(&gone, &check(&["claude-acp"], &[])),
        Decision::Denied
    );
    assert_eq!(gone[0].outcome, Outcome::Gone);
}

#[test]
fn another_builds_item_and_a_locked_keychain_would_ask() {
    // Probed on a Mac with dialogs off: both answer errSecAuthFailed.
    assert_eq!(silent_answer(Err(AUTH_FAILED)), Some(false));
    assert_eq!(silent_answer(Err(-25293)), Some(false));
}

#[test]
fn a_dialog_that_could_not_show_would_ask() {
    assert_eq!(silent_answer(Err(INTERACTION_NOT_ALLOWED)), Some(false));
    assert_eq!(silent_answer(Err(IN_DARK_WAKE)), Some(false));
}

#[test]
fn a_storage_access_refusal_would_ask_and_is_never_gone() {
    // errSecNoStorageAccess: refused, not gone.
    assert_eq!(silent_answer(Err(-25244)), Some(false));
}

#[test]
fn a_silent_read_that_reads_needs_no_asking() {
    assert_eq!(silent_answer(Ok(true)), Some(true));
}

#[test]
fn an_item_gone_since_it_was_listed_needs_no_asking() {
    // As the Ask counts it: nothing left to ask for.
    assert_eq!(silent_answer(Err(ITEM_NOT_FOUND)), Some(true));
}

#[test]
fn any_other_answer_cannot_be_told() {
    // errSecNotAvailable, errSecDuplicateItem, and an answer with no value.
    for read in [Err(-25291), Err(-25299), Ok(false)] {
        assert_eq!(silent_answer(read), None, "{read:?}");
    }
}

#[test]
fn an_item_that_would_ask_shows_would_ask_even_after_a_grant() {
    let found = check(&["claude-acp"], &[]);
    assert_eq!(
        keychain_state(&found, None),
        Some(OsPermissionState::WouldAsk)
    );
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
fn an_item_that_cannot_be_told_shows_unknown_never_granted() {
    let found = check(&[], &["files"]);
    assert_eq!(
        keychain_state(&found, None),
        Some(OsPermissionState::Unknown)
    );
    assert_eq!(
        keychain_state(&found, Some(Answer::Granted)),
        Some(OsPermissionState::Unknown)
    );
    assert_eq!(
        keychain_state(&found, Some(Answer::Denied)),
        Some(OsPermissionState::Denied)
    );
}

#[test]
fn the_row_says_whether_this_build_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut probe = mac();
    probe.keychain = check(&["claude-acp"], &[]);
    assert!(!rows(&probe, &store(&dir, "1"))[0].asked);
    ask(&probe, &store(&dir, "1"), PermissionId::Keychain, &|| {}).unwrap();
    assert!(rows(&probe, &store(&dir, "1"))[0].asked);
    assert!(!rows(&probe, &store(&dir, "2"))[0].asked);
}
