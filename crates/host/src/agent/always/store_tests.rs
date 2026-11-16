//! The kept-rules file (`store.rs`): beside the matter's notes, signed for this host and this
//! matter, set aside when bad, deleted on forget in order with pending adds.

use serde_json::{Value, json};

use clauseo_connectors::SecretStore as _;

use super::store::{read, remove_now, store_now};
use super::tests::{in_matter, keyed};
use super::{FILE, file_beside};
use crate::pipeline::tests::support::TempDir;

#[test]
fn the_file_sits_beside_the_notes_and_keeps_each_rule_once() {
    keyed();
    let dir = TempDir::new("always_file");
    let notes = dir.path().join("agent-state").join("m1").join("notes");
    crate::write::create_dir_all(&notes).unwrap();
    let file = file_beside(&notes).unwrap();
    assert_eq!(file, dir.path().join("agent-state").join("m1").join(FILE));
    assert_eq!(read(&file), Vec::<String>::new());
    store_now(&file, "Bash(wc -l notes.txt)".to_owned(), 1).unwrap();
    store_now(&file, "Bash(wc -l notes.txt)".to_owned(), 2).unwrap();
    store_now(&file, "WebFetch(domain:sci.gov.in)".to_owned(), 3).unwrap();
    store_now(&file, "Bash(*)".to_owned(), 4).unwrap();
    assert_eq!(
        read(&file),
        ["Bash(wc -l notes.txt)", "WebFetch(domain:sci.gov.in)"]
    );
    // Nothing but the file was made, and nothing in the notes.
    assert_eq!(std::fs::read_dir(&notes).unwrap().count(), 0);
}

/// A file this host did not write for this matter holds nothing: edited by hand, planted,
/// version 1, copied from another matter, or signed under another key.
#[test]
fn a_file_without_this_hosts_mac_for_this_matter_holds_nothing() {
    let store = keyed();
    let dir = TempDir::new("always_planted");
    let file = in_matter(&dir, "m1");
    store_now(&file, "Bash(ls)".to_owned(), 1).unwrap();
    assert_eq!(read(&file), ["Bash(ls)"]);
    let good = std::fs::read_to_string(&file).unwrap();
    let mut value: Value = serde_json::from_str(&good).unwrap();
    assert_eq!(value["version"], json!(2));
    // A rule added by hand, or one removed by hand: the MAC no longer fits.
    value["allow"] = json!(["Bash(ls)", "Bash(ls -l)"]);
    crate::write::write_file(&file, value.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    value["allow"] = json!([]);
    crate::write::write_file(&file, value.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    // Version 1, as #1109 wrote it.
    let v1 = json!({"version": 1, "allow": ["Bash(ls)"]});
    crate::write::write_file(&file, v1.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    // The same file copied into another matter: its MAC names the first.
    crate::write::write_file(&file, good.as_bytes()).unwrap();
    assert_eq!(read(&file), ["Bash(ls)"]);
    let other = in_matter(&dir, "m2");
    crate::write::write_file(&other, good.as_bytes()).unwrap();
    assert_eq!(read(&other), Vec::<String>::new());
    // Another key (another machine's Keychain) makes another MAC.
    store.set(super::store::KEY_ACCOUNT, &[7_u8; 32]).unwrap();
    super::use_secrets(Some(store));
    assert_eq!(read(&file), Vec::<String>::new());
    // Not JSON at all.
    crate::write::write_file(&file, b"not json").unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
}

/// Without a secret store nothing is read, kept or offered.
#[test]
fn no_secret_store_keeps_nothing() {
    super::use_secrets(None);
    let dir = TempDir::new("always_no_store");
    let notes = dir.path().join("agent-state").join("m1").join("notes");
    crate::write::create_dir_all(&notes).unwrap();
    assert_eq!(super::read_beside(&notes), None);
    let file = file_beside(&notes).unwrap();
    assert!(store_now(&file, "Bash(ls)".to_owned(), 1).is_err());
    assert!(!file.exists());
    let store = keyed();
    assert_eq!(super::read_beside(&notes), Some(Vec::new()));
    // The key is made once and kept in the store.
    let key = store.get(super::store::KEY_ACCOUNT).unwrap().unwrap();
    assert_eq!(key.len(), 32);
}

/// A forgotten matter's file is deleted; a missing one is fine.
#[test]
fn a_forgotten_matters_file_is_deleted() {
    keyed();
    let dir = TempDir::new("always_forget");
    let file = in_matter(&dir, "m1");
    store_now(&file, "Bash(ls)".to_owned(), 1).unwrap();
    super::store::delete_now(&file, 2).unwrap();
    assert!(!file.exists());
    super::store::delete_now(&file, 3).unwrap();
}

/// An add the lawyer answered before forgetting the matter, whose write reaches the lock after
/// the deletion, keeps nothing: no rule outlives its matter. A file set aside goes too.
#[test]
fn an_add_pending_when_the_matter_is_forgotten_keeps_nothing() {
    keyed();
    let dir = TempDir::new("always_forget_race");
    let file = in_matter(&dir, "m1");
    crate::write::write_file(&file, b"not this host's").unwrap();
    store_now(&file, "Bash(ls)".to_owned(), 1).unwrap();
    let aside = file.with_file_name(format!("{FILE}.bad-1"));
    assert!(aside.exists());
    super::store::delete_now(&file, 3).unwrap();
    assert!(!aside.exists(), "a set-aside file is the matter's too");
    store_now(&file, "Bash(ls -l)".to_owned(), 2).unwrap();
    assert!(
        !file.exists(),
        "an add numbered before the forget wrote the file back"
    );
    assert_eq!(read(&file), Vec::<String>::new());
}

/// A key of the wrong length in the secret store is never replaced (that would void every
/// matter's rules): nothing is read, kept or offered until it is dealt with.
#[test]
fn a_malformed_key_is_not_replaced() {
    let store = keyed();
    store.set(super::store::KEY_ACCOUNT, &[7_u8; 16]).unwrap();
    let dir = TempDir::new("always_bad_key");
    let notes = dir.path().join("agent-state").join("m1").join("notes");
    crate::write::create_dir_all(&notes).unwrap();
    assert_eq!(super::read_beside(&notes), None);
    let file = file_beside(&notes).unwrap();
    assert!(store_now(&file, "Bash(ls)".to_owned(), 1).is_err());
    let kept = store.get(super::store::KEY_ACCOUNT).unwrap().unwrap();
    assert_eq!(kept, [7_u8; 16]);
}

/// A file this host cannot take is never written over: the next write sets it aside as
/// `.bad-<n>` and starts empty; a removal leaves it alone.
#[test]
fn a_bad_file_is_set_aside_not_overwritten() {
    keyed();
    let dir = TempDir::new("always_bad");
    let file = dir.path().join(FILE);
    crate::write::write_file(&file, b"not json").unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    assert!(!remove_now(&file, "Bash(ls)", 5).unwrap());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not json");
    store_now(&file, "Bash(ls)".to_owned(), 6).unwrap();
    let aside = dir.path().join(format!("{FILE}.bad-1"));
    assert_eq!(std::fs::read_to_string(&aside).unwrap(), "not json");
    assert_eq!(read(&file), ["Bash(ls)"]);
    crate::write::write_file(&file, &vec![b' '; (1 << 20) + 1]).unwrap();
    store_now(&file, "Bash(ls -l)".to_owned(), 7).unwrap();
    assert!(dir.path().join(format!("{FILE}.bad-2")).exists());
    assert_eq!(read(&file), ["Bash(ls -l)"]);
}

/// The lawyer's removal takes one rule out and keeps the rest.
#[test]
fn a_removed_rule_is_gone_and_the_rest_stay() {
    keyed();
    let dir = TempDir::new("always_remove");
    let file = dir.path().join(FILE);
    store_now(&file, "Bash(ls)".to_owned(), 8).unwrap();
    store_now(&file, "Bash(ls -l)".to_owned(), 9).unwrap();
    assert!(remove_now(&file, "Bash(ls)", 10).unwrap());
    assert!(!remove_now(&file, "Bash(ls)", 11).unwrap());
    assert_eq!(read(&file), ["Bash(ls -l)"]);
}

/// An add whose write was still pending when the lawyer removed its rule runs after the removal
/// and never puts the rule back; an add the lawyer chose after the removal does.
#[test]
fn a_pending_add_never_undoes_a_removal() {
    keyed();
    let dir = TempDir::new("always_pending");
    let file = dir.path().join(FILE);
    store_now(&file, "Bash(ls -l)".to_owned(), 1).unwrap();
    // The add numbered 2 reaches the lock only after the removal numbered 3.
    assert!(!remove_now(&file, "Bash(ls)", 3).unwrap());
    store_now(&file, "Bash(ls)".to_owned(), 2).unwrap();
    assert_eq!(read(&file), ["Bash(ls -l)"]);
    // A removal that comes after a later add of the same rule leaves it.
    store_now(&file, "Bash(ls)".to_owned(), 5).unwrap();
    assert!(!remove_now(&file, "Bash(ls)", 4).unwrap());
    assert_eq!(read(&file), ["Bash(ls -l)", "Bash(ls)"]);
}
