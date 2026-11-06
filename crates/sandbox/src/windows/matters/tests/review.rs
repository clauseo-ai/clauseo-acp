//! The S15 `PR4c` review's defects, each as a test that failed before its fix.

use super::pool::{full, ledger_in, ledger_naming, ledger_of, pool, reread, sids_in, user};
use super::*;
use crate::windows::pool::assign::Why;
use crate::windows::set_aside::{SET_ASIDE_DIR, open_session};

/// Writes `matter`'s set-aside manifest as `json`.
fn manifest(temp: &TempDir, matter: MatterId, json: &str) {
    temp.file(
        &format!("{SANDBOX_DIR}/{SET_ASIDE_DIR}/{matter}/manifest.json"),
        json,
    );
}

/// A manifest with one finished entry for an item at `to` owned by `owner` (a JSON value).
fn one_entry(matter: MatterId, to: &Path, owner: &str) -> String {
    let to = serde_json::to_string(to.to_str().unwrap()).unwrap();
    format!(
        r#"{{"version":1,"next":2,"entries":[{{"id":1,"matter":"{matter}","from":"x","to":{to},
        "folder":"f","within":[],"name":".git","reason":"new","owner":{owner},"at":"t"}}]}}"#
    )
}

/// High: a pool user owning an item set aside and still on disk is held back; once the item is
/// gone it is free. An unreadable manifest, or an item whose owner wasn't read, holds every
/// user; a move begun holds its owner whether its item is there or not.
#[test]
fn a_user_owning_a_set_aside_item_is_held_back() {
    let temp = TempDir::new("review-set-aside");
    let records = TempDir::new("review-set-aside-ledger");
    let ledger = ledger_in(records.path(), &pool(2));
    let (gone, new) = (matter(), matter());
    let item = temp.file("item/.git", "x");
    let owner = format!(r#""{}""#, user(1).as_str().to_ascii_lowercase());
    manifest(&temp, gone, &one_entry(gone, &item, &owner));
    let none = OpenSessions::default();
    let mut sids = sids_in(&temp);
    let got = sids.assign(&new, &pool(1), reread(&ledger), &none);
    assert_eq!(got, Err(full(1, vec![(user(1), Why::SetAside)])));
    // Another user is free.
    assert_eq!(
        sids.assign(&new, &pool(2), reread(&ledger), &none),
        Ok(user(2))
    );
    let other = matter();
    write::remove_owned_file(&item).unwrap();
    assert_eq!(
        sids.assign(&other, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );

    // Unreadable, or an owner not read: every user is held.
    let temp = TempDir::new("review-set-aside-closed");
    let item = temp.file("item/.git", "x");
    let both = vec![(user(1), Why::SetAside), (user(2), Why::SetAside)];
    for bad in [
        "not json".to_owned(),
        r#"{"version":2,"next":1,"entries":[]}"#.to_owned(),
        one_entry(gone, &item, "null"),
        one_entry(gone, &item, "7"),
    ] {
        manifest(&temp, gone, &bad);
        let got = sids_in(&temp).assign(&new, &pool(2), reread(&ledger), &none);
        assert_eq!(got, Err(full(2, both.clone())), "{bad}");
    }

    // A move begun holds its owner though its item is at neither end.
    let begun = format!(
        r#"{{"version":1,"next":2,"entries":[],"intents":[{{"to":"/nowhere/x","owner":"{}"}}]}}"#,
        user(1)
    );
    manifest(&temp, gone, &begun);
    let got = sids_in(&temp).assign(&new, &pool(1), reread(&ledger), &none);
    assert_eq!(got, Err(full(1, vec![(user(1), Why::SetAside)])));
}

/// Medium: `assign` reads the ledger inside the lock, so an entry recorded after the caller's
/// own read is seen; and `release` refuses while the matter has an open session.
#[test]
fn assign_reads_the_ledger_inside_the_lock_and_release_waits_for_sessions() {
    let temp = TempDir::new("review-lock");
    let records = TempDir::new("review-lock-ledger");
    let stale = ledger_in(records.path(), &pool(1));
    let (one, new) = (matter(), matter());
    // Recorded after `stale` was read.
    ledger_naming(&records, one, &user(1));
    assert!(!stale.names_sid(&user(1)), "the caller's read is stale");
    let got = sids_in(&temp).assign(&new, &pool(1), reread(&stale), &OpenSessions::default());
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Entries)])));
    // A loader that fails refuses the assignment.
    let failing = || Err(io::Error::other("no ledger"));
    let got = sids_in(&temp).assign(&new, &pool(1), failing, &OpenSessions::default());
    assert!(matches!(got, Err(Assign::Records(_))), "{got:?}");

    let empty = TempDir::new("review-lock-empty");
    let empty = ledger_in(empty.path(), &pool(1));
    let mut sids = sids_in(&temp);
    let none = OpenSessions::default();
    assert_eq!(
        sids.assign(&one, &pool(1), reread(&empty), &none),
        Ok(user(1))
    );
    open_session(temp.path(), &one, "s1");
    assert!(sids.release(&one, UNIX_EPOCH).is_err());
    assert_eq!(sids_in(&temp).pool_user(&one), Some(&user(1)), "still held");
}

/// Low: a released matter assigned again keeps its earlier user in `held_before`, and its open
/// session still holds that user back.
#[test]
fn a_matters_earlier_users_stay_held_by_its_open_session() {
    let temp = TempDir::new("review-before");
    let records = TempDir::new("review-before-ledger");
    let ledger = ledger_in(records.path(), &pool(2));
    let (one, new) = (matter(), matter());
    let none = OpenSessions::default();
    let mut sids = sids_in(&temp);
    assert_eq!(
        sids.assign(&one, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
    sids.release(&one, UNIX_EPOCH).unwrap();
    assert_eq!(
        sids.assign(&one, &pool(2), reread(&ledger), &none),
        Ok(user(2))
    );
    let row = &sids.rows()[&one];
    assert_eq!(
        (row.pool_user.clone(), row.held_before.clone()),
        (Some(user(2)), vec![user(1)])
    );
    let text = std::fs::read_to_string(temp.path().join(SANDBOX_DIR).join(MATTERS_FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json["matters"][one.to_string()]["held_before"][0],
        user(1).as_str()
    );
    let open = OpenSessions::new([one]);
    let got = sids.assign(&new, &pool(2), reread(&ledger), &open);
    let want = full(2, vec![(user(1), Why::Open), (user(2), Why::Held(one))]);
    assert_eq!(got, Err(want));
}

/// Low: a kept hand-back naming a user under any key holds it back through `names_sid`, though
/// `handback_owed` only matches its `pool` key.
#[test]
fn a_kept_handback_naming_a_user_under_another_key_holds_it_back() {
    let temp = TempDir::new("review-kept-handback");
    let records = TempDir::new("review-kept-handback-ledger");
    let json = format!(
        r#"{{"version":1,"entries":[],"handbacks":[{{"matter":"x","pools":["{}"]}}]}}"#,
        user(1)
    );
    let ledger = ledger_of(&records, &json);
    assert!(!ledger.handback_owed(&user(1)));
    assert!(ledger.names_sid(&user(1)));
    let got = sids_in(&temp).assign(
        &matter(),
        &pool(1),
        reread(&ledger),
        &OpenSessions::default(),
    );
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Entries)])));
}
