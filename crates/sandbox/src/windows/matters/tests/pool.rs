//! Pool users in `matters.json`: assignment, the reuse rule, release, and the v2 rows.

use std::path::Path;

use super::*;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use crate::windows::admit::PathKind;
use crate::windows::ledger::ledger_path;
use crate::windows::pool::assign::{HeldBack, Why};

/// Pool user `n`'s SID in these tests.
fn user(n: u32) -> Sid {
    Sid::parse(&format!(
        "S-1-5-21-1111111111-2222222222-3333333333-{}",
        1000 + n
    ))
    .unwrap()
}

fn pool(size: u32) -> Vec<Sid> {
    (1..=size).map(user).collect()
}

/// The ledger at `dir`, trusting `trusted`.
fn ledger_in(dir: &Path, trusted: &[Sid]) -> Ledger {
    Ledger::open(ledger_path(dir), trusted, &Reserved::default()).unwrap()
}

/// A ledger whose file is `json`, in a folder of its own, trusting the pool of 4.
fn ledger_of(temp: &TempDir, json: &str) -> Ledger {
    temp.file(&format!("{SANDBOX_DIR}/acl-ledger.json"), json);
    ledger_in(temp.path(), &pool(4))
}

/// A ledger with one trusted entry naming `sid`, for `matter`.
fn ledger_naming(temp: &TempDir, matter: MatterId, sid: &Sid) -> Ledger {
    let mut ledger = ledger_in(temp.path(), &pool(4));
    let grant = Grant {
        path: std::env::temp_dir().join("clauseo-pool-paths").join("m"),
        sid: sid.clone(),
        rights: Rights::Modify,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    let mut plan = AdmittedPlan::from_grants(matter, vec![grant]);
    ledger.record_pending(&mut plan).unwrap();
    ledger
}

/// A ledger owing a hand-back for `sid`: trusted when `matter` is a matter id, kept when not.
fn ledger_owing(temp: &TempDir, matter: &str, sid: &Sid) -> Ledger {
    let json = format!(
        r#"{{"version":1,"entries":[],"handbacks":[{{"matter":"{matter}","pool":"{sid}"}}]}}"#
    );
    ledger_of(temp, &json)
}

fn sids_in(temp: &TempDir) -> MatterSids {
    MatterSids::load_with(temp.path(), Reserved::default()).unwrap()
}

/// `secs` seconds after the Unix epoch.
fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn none_open() -> OpenSessions {
    OpenSessions::default()
}

fn full(size: usize, held_back: Vec<(Sid, Why)>) -> Assign {
    let held_back = held_back
        .into_iter()
        .map(|(pool, why)| HeldBack { pool, why })
        .collect();
    Assign::PoolFull { size, held_back }
}

#[test]
fn each_reason_holds_a_user_back() {
    let (one, two, new) = (matter(), matter(), matter());
    let pool = pool(1);
    let empty = TempDir::new("pool-reasons-empty");
    let empty = ledger_in(empty.path(), &pool);

    // Held: another matter holds it.
    let temp = TempDir::new("pool-reasons-held");
    let mut sids = sids_in(&temp);
    sids.assign(&one, &pool, &empty, &none_open()).unwrap();
    let got = sids.assign(&new, &pool, &empty, &none_open());
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Held(one))])));

    // Open: released, but the matter that held it has an open session.
    sids.release(&one, at(10)).unwrap();
    let open = OpenSessions::new([one]);
    let got = sids.assign(&new, &pool, &empty, &open);
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Open)])));
    // Another matter's open session doesn't hold it.
    let other_open = OpenSessions::new([two]);
    assert_eq!(sids.assign(&new, &pool, &empty, &other_open), Ok(user(1)));

    // Owed: a hand-back is owed for it.
    let temp = TempDir::new("pool-reasons-owed");
    let owed = TempDir::new("pool-reasons-owed-ledger");
    let ledger = ledger_owing(&owed, &one.to_string(), &user(1));
    let got = sids_in(&temp).assign(&new, &pool, &ledger, &none_open());
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Owed)])));

    // Entries: a ledger entry names it, in any state.
    let temp = TempDir::new("pool-reasons-entries");
    let named = TempDir::new("pool-reasons-entries-ledger");
    let ledger = ledger_naming(&named, two, &user(1));
    let got = sids_in(&temp).assign(&new, &pool, &ledger, &none_open());
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Entries)])));

    // Entries: a kept entry this version can't read names it too.
    let kept = TempDir::new("pool-reasons-kept-entry");
    let ledger = ledger_of(
        &kept,
        &format!(
            r#"{{"version":1,"entries":[{{"id":7,"sid":"{}"}}]}}"#,
            user(1)
        ),
    );
    assert_eq!(ledger.set_aside_entries(), 1);
    let got = sids_in(&temp).assign(&new, &pool, &ledger, &none_open());
    assert_eq!(got, Err(full(1, vec![(user(1), Why::Entries)])));
    assert!(
        sids_in(&temp).is_empty(),
        "nothing is written when the pool is full"
    );
}

#[test]
fn a_kept_owed_entry_holds_a_user_back() {
    let temp = TempDir::new("pool-kept-owed");
    let owed = TempDir::new("pool-kept-owed-ledger");
    let ledger = ledger_owing(&owed, "not a matter id", &user(2));
    assert!(
        ledger.handbacks_owed().is_empty(),
        "the hand-back is kept, not trusted"
    );
    let mut sids = sids_in(&temp);
    let first = sids.assign(&matter(), &pool(2), &ledger, &none_open());
    assert_eq!(first, Ok(user(1)));
    let got = sids.assign(&matter(), &pool(2), &ledger, &none_open());
    let held = sids.rows().keys().next().copied().unwrap();
    let want = full(2, vec![(user(1), Why::Held(held)), (user(2), Why::Owed)]);
    assert_eq!(got, Err(want));
}

#[test]
fn a_released_user_goes_to_another_matter() {
    let temp = TempDir::new("pool-release");
    let empty = TempDir::new("pool-release-ledger");
    let ledger = ledger_in(empty.path(), &pool(1));
    let (one, two) = (matter(), matter());
    let mut sids = sids_in(&temp);
    assert_eq!(
        sids.assign(&one, &pool(1), &ledger, &none_open()),
        Ok(user(1))
    );
    assert!(sids.assign(&two, &pool(1), &ledger, &none_open()).is_err());
    assert_eq!(sids.release(&one, at(1)), Ok(true));
    assert_eq!(sids.release(&one, at(1)), Ok(false), "released already");
    assert_eq!(sids.release(&matter(), at(1)), Ok(false), "no such matter");
    assert_eq!(sids.pool_user(&one), None);
    let reloaded = sids_in(&temp);
    let row = &reloaded.rows()[&one];
    assert_eq!(
        row.pool_user,
        Some(user(1)),
        "the released row still names the user"
    );
    assert_eq!(row.released_at, Some(1));
    assert_eq!(
        sids.assign(&two, &pool(1), &ledger, &none_open()),
        Ok(user(1))
    );
    let again = sids_in(&temp);
    assert_eq!(
        (again.pool_user(&one), again.pool_user(&two)),
        (None, Some(&user(1)))
    );
}

#[test]
fn never_used_users_go_first_then_the_longest_free() {
    let temp = TempDir::new("pool-order");
    let empty = TempDir::new("pool-order-ledger");
    let ledger = ledger_in(empty.path(), &pool(3));
    let mut sids = sids_in(&temp);
    let (first, second, third) = (matter(), matter(), matter());
    let (fourth, fifth) = (matter(), matter());
    for (m, want) in [(first, 1), (second, 2)] {
        assert_eq!(
            sids.assign(&m, &pool(3), &ledger, &none_open()),
            Ok(user(want))
        );
    }
    sids.release(&second, at(100)).unwrap();
    sids.release(&first, at(200)).unwrap();
    // User 3 was never used, so it goes before both released ones.
    assert_eq!(
        sids.assign(&third, &pool(3), &ledger, &none_open()),
        Ok(user(3))
    );
    // Then user 2, free since 100, before user 1, free since 200.
    assert_eq!(
        sids.assign(&fourth, &pool(3), &ledger, &none_open()),
        Ok(user(2))
    );
    assert_eq!(
        sids.assign(&fifth, &pool(3), &ledger, &none_open()),
        Ok(user(1))
    );
}

#[test]
fn a_full_pool_names_every_user_held_back_and_why() {
    let temp = TempDir::new("pool-full");
    let records = TempDir::new("pool-full-ledger");
    let (holder, gone, forgotten, new) = (matter(), matter(), matter(), matter());
    // User 3 named by an entry, user 2 owed.
    let mut ledger = ledger_naming(&records, gone, &user(3));
    let owed = format!(r#"[{{"matter":"{forgotten}","pool":"{}"}}]"#, user(2));
    let path = ledger_path(records.path());
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    json["handbacks"] = serde_json::from_str(&owed).unwrap();
    write::write_atomic(&path, &serde_json::to_vec(&json).unwrap()).unwrap();
    ledger = Ledger::open(ledger.path(), &pool(4), &Reserved::default()).unwrap();
    let empty = TempDir::new("pool-full-empty");
    let empty = ledger_in(empty.path(), &pool(4));
    let mut sids = sids_in(&temp);
    // User 1 held; user 4 held, then released by a matter with an open session.
    assert_eq!(
        sids.assign(&holder, &pool(1), &empty, &none_open()),
        Ok(user(1))
    );
    let pool4 = [user(4)];
    assert_eq!(
        sids.assign(&gone, &pool4, &empty, &none_open()),
        Ok(user(4))
    );
    sids.release(&gone, at(5)).unwrap();
    let got = sids.assign(&new, &pool(4), &ledger, &OpenSessions::new([gone]));
    let want = full(
        4,
        vec![
            (user(1), Why::Held(holder)),
            (user(2), Why::Owed),
            (user(3), Why::Entries),
            (user(4), Why::Open),
        ],
    );
    assert_eq!(got, Err(want));
    assert_eq!(sids.pool_user(&new), None);
}

#[test]
fn a_version_1_file_is_upgraded_on_the_next_write() {
    let temp = TempDir::new("pool-v1");
    let a = matter();
    let sid = "S-1-5-21-1-2-3-1000";
    let v1 = format!(r#"{{"version":1,"matters":{{"{a}":"{sid}"}}}}"#);
    let path = temp.file(&format!("{SANDBOX_DIR}/{MATTERS_FILE}"), &v1);
    let mut sids = sids_in(&temp);
    assert_eq!(sids.get(&a).map(Sid::as_str), Some(sid));
    assert_eq!(sids.pool_user(&a), None);
    // Reading the SID writes nothing.
    assert_eq!(sids.sid_for(&a).unwrap().as_str(), sid);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), v1);
    let empty = TempDir::new("pool-v1-ledger");
    let ledger = ledger_in(empty.path(), &pool(1));
    assert_eq!(
        sids.assign(&a, &pool(1), &ledger, &none_open()),
        Ok(user(1))
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["version"], 2);
    let row = &json["matters"][a.to_string()];
    assert_eq!(row["sid"], sid);
    assert_eq!(row["pool_user"], user(1).as_str());
    assert!(row.get("released_at").is_none());
}

#[test]
fn an_unreadable_file_is_refused_and_left_as_it_is() {
    let temp = TempDir::new("pool-bad");
    let empty = TempDir::new("pool-bad-ledger");
    let ledger = ledger_in(empty.path(), &pool(2));
    let (a, b) = (matter(), matter());
    let (u1, u2) = (user(1), user(2));
    let v2 = |rows: String| format!(r#"{{"version":2,"matters":{{{rows}}}}}"#);
    for bad in [
        "not json".to_owned(),
        r#"{"version":3,"matters":{}}"#.to_owned(),
        format!(r#"{{"version":2,"matters":{{"{a}":{{}}}}}}"#),
        format!(r#"{{"version":2,"matters":{{"{a}":{{"released_at":5}}}}}}"#),
        format!(r#"{{"version":2,"matters":{{"{a}":{{"pool_user":"{u1}","x":1}}}}}}"#),
        format!(r#"{{"version":2,"matters":{{"{a}":{{"pool_user":"S-1-0"}}}}}}"#),
        // One pool user held by two matters.
        v2(format!(
            r#""{a}":{{"pool_user":"{u1}"}},"{b}":{{"pool_user":"{u1}"}}"#
        )),
        // One SID both a matter's SID and a pool user.
        v2(format!(
            r#""{a}":{{"sid":"{u2}","pool_user":"{u1}"}},"{b}":{{"pool_user":"{u2}"}}"#
        )),
        format!(r#"{{"version":1,"matters":{{"{a}":{{"pool_user":"{u1}"}}}}}}"#),
    ] {
        let path = temp.file(&format!("{SANDBOX_DIR}/{MATTERS_FILE}"), &bad);
        assert!(MatterSids::load(temp.path()).is_err(), "{bad}");
        // Loaded before the file went bad, so each change meets it on its fresh read.
        let mut sids = MatterSids {
            folder: temp.path().join(SANDBOX_DIR),
            reserved: Reserved::default(),
            rows: Rows::new(),
            lock_wait: None,
        };
        let got = sids.assign(&matter(), &pool(2), &ledger, &none_open());
        assert!(matches!(got, Err(Assign::Records(_))), "{bad}: {got:?}");
        assert!(sids.release(&a, at(1)).is_err(), "{bad}");
        assert!(sids.sid_for(&a).is_err(), "{bad}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
}

#[test]
fn a_matter_keeps_its_user_across_calls() {
    let temp = TempDir::new("pool-keep");
    let records = TempDir::new("pool-keep-ledger");
    let a = matter();
    let empty = ledger_in(records.path(), &pool(3));
    let mut sids = sids_in(&temp);
    assert_eq!(sids.assign(&a, &pool(3), &empty, &none_open()), Ok(user(1)));
    let path = temp.path().join(SANDBOX_DIR).join(MATTERS_FILE);
    let before = std::fs::read(&path).unwrap();
    // Its own entries, an open session of its own and a reload don't move it.
    let ledger = ledger_naming(&records, a, &user(1));
    let open = OpenSessions::new([a]);
    assert_eq!(sids.assign(&a, &pool(3), &ledger, &open), Ok(user(1)));
    let mut again = sids_in(&temp);
    assert_eq!(again.pool_user(&a), Some(&user(1)));
    assert_eq!(again.assign(&a, &pool(3), &ledger, &open), Ok(user(1)));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "keeping writes nothing"
    );
    // A SID made later keeps the pool user in the same row.
    let sid = again.sid_for(&a).unwrap();
    let reloaded = sids_in(&temp);
    let row = &reloaded.rows()[&a];
    assert_eq!(
        (row.sid.as_ref(), row.pool_user.as_ref()),
        (Some(&sid), Some(&user(1)))
    );
    // A user the machine's pool no longer lists is refused, not swapped.
    let got = again.assign(&a, &pool(3)[1..], &ledger, &open);
    assert!(matches!(got, Err(Assign::Records(_))), "{got:?}");
}
