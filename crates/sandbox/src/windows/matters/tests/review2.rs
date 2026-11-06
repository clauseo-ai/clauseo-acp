//! The S15 `PR4c` delta review's defects, each as a test that failed before its fix.

use super::pool::{Launch, full, ledger_in, pool, reread, sids_in, user};
use super::*;
use crate::windows::pool::assign::Why;
use crate::windows::set_aside::{SET_ASIDE_DIR, open_session};

/// `matter`'s set-aside folder, relative to the data folder.
fn folder(matter: MatterId) -> String {
    format!("{SANDBOX_DIR}/{SET_ASIDE_DIR}/{matter}")
}

/// A manifest with one finished entry for the item at `to`, recorded as the lawyer's.
fn lawyers_item(matter: MatterId, to: &Path) -> String {
    let to = serde_json::to_string(to.to_str().unwrap()).unwrap();
    format!(
        r#"{{"version":1,"next":2,"entries":[{{"id":1,"matter":"{matter}","from":"x","to":{to},
        "folder":"f","within":[],"name":".git","reason":"new","owner":"{}","at":"t"}}]}}"#,
        super::pool::LAWYER
    )
}

/// High: a folder the manifest records as the lawyer's holds back a pool user owning anything
/// inside it; a child whose owner can't be read holds every user.
#[test]
fn a_user_owning_anything_inside_a_set_aside_item_is_held_back() {
    let temp = TempDir::new("review2-tree");
    let records = TempDir::new("review2-tree-ledger");
    let ledger = ledger_in(records.path(), &pool(3));
    let (gone, new) = (matter(), matter());
    let pass = format!("{}/2026-11-06T00-00-00Z", folder(gone));
    let item = temp.path().join(&pass).join(".git");
    temp.file(&format!("{pass}/.git/objects/pool-2"), "x");
    temp.file(
        &format!("{}/manifest.json", folder(gone)),
        &lawyers_item(gone, &item),
    );
    let none = OpenSessions::default();
    // User 1 owns nothing set aside; user 2 owns a file inside the lawyer's folder.
    assert_eq!(
        sids_in(&temp).launch(&new, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
    let other = matter();
    let got = sids_in(&temp).launch(&other, &pool(3)[1..], reread(&ledger), &none);
    assert_eq!(got, Ok(user(3)), "user 2 is held back");
    let got = sids_in(&temp).launch(&matter(), &pool(3)[1..2], reread(&ledger), &none);
    assert_eq!(got, Err(full(1, vec![(user(2), Why::SetAside)])));

    // A child whose owner can't be read holds every user.
    temp.file(&format!("{pass}/.git/unreadable"), "x");
    let third = matter();
    let got = sids_in(&temp).launch(&third, &pool(4)[3..], reread(&ledger), &none);
    assert_eq!(got, Err(full(1, vec![(user(4), Why::SetAside)])));
}

/// High: a link inside an item counts by its own owner, and is not followed.
#[cfg(unix)]
#[test]
fn the_walk_follows_no_link() {
    let temp = TempDir::new("review2-link");
    let records = TempDir::new("review2-link-ledger");
    let ledger = ledger_in(records.path(), &pool(2));
    let gone = matter();
    let pass = format!("{}/2026-11-06T00-00-00Z", folder(gone));
    let item = temp.path().join(&pass).join(".git");
    temp.file(&format!("{pass}/.git/a"), "x");
    let elsewhere = temp.file("elsewhere/pool-2", "x");
    temp.symlink(&format!("{pass}/.git/pool-1"), elsewhere.parent().unwrap());
    temp.file(
        &format!("{}/manifest.json", folder(gone)),
        &lawyers_item(gone, &item),
    );
    let none = OpenSessions::default();
    let got = sids_in(&temp).launch(&matter(), &pool(2), reread(&ledger), &none);
    assert_eq!(
        got,
        Ok(user(2)),
        "the link's own owner holds user 1; user 2 is past it"
    );
}

/// Low: a set-aside folder holding a pass folder but no manifest holds every user; one with
/// only its lock and record holds none.
#[test]
fn items_with_no_manifest_hold_every_user() {
    let temp = TempDir::new("review2-no-manifest");
    let records = TempDir::new("review2-no-manifest-ledger");
    let ledger = ledger_in(records.path(), &pool(1));
    let (gone, new) = (matter(), matter());
    temp.file(&format!("{}/set-aside.lock", folder(gone)), "");
    let none = OpenSessions::default();
    assert_eq!(
        sids_in(&temp).launch(&new, &pool(1), reread(&ledger), &none),
        Ok(user(1))
    );
    temp.file(&format!("{}/2026-11-06T00-00-00Z/.git", folder(gone)), "x");
    let got = sids_in(&temp).launch(&matter(), &pool(2)[1..], reread(&ledger), &none);
    assert_eq!(got, Err(full(1, vec![(user(2), Why::SetAside)])));
}

/// Medium: a new pick needs the matter's session marked open first; a kept user doesn't.
#[test]
fn assign_refuses_a_new_pick_for_a_matter_with_no_open_session() {
    let temp = TempDir::new("review2-session");
    let records = TempDir::new("review2-session-ledger");
    let ledger = ledger_in(records.path(), &pool(1));
    let one = matter();
    let none = OpenSessions::default();
    let mut sids = sids_in(&temp);
    let got = sids.assign(&one, &pool(1), reread(&ledger), &none);
    assert!(matches!(got, Err(Assign::Records(_))), "{got:?}");
    assert_eq!(sids_in(&temp).pool_user(&one), None);
    open_session(temp.path(), &one, "s1");
    assert_eq!(
        sids.assign(&one, &pool(1), reread(&ledger), &none),
        Ok(user(1))
    );
    super::pool::close_sessions(temp.path(), &one);
    assert_eq!(
        sids.assign(&one, &pool(1), reread(&ledger), &none),
        Ok(user(1)),
        "kept"
    );
}
