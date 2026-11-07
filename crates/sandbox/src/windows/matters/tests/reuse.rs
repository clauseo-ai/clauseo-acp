//! The reuse rule against what a real fence writes: each matter's sibling denies name every other
//! pool user, and grant none of them anything; a released user waits for its profile reset.

use super::pool::{Launch, full, ledger_in, ledger_owing, pool, reread, sids_in, user};
use super::*;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use crate::windows::admit::PathKind;
use crate::windows::ledger::EntryState;
use crate::windows::pool::assign::Why;

/// A folder of `matter`'s, as its grants name it.
fn folder(matter: &MatterId) -> PathBuf {
    std::env::temp_dir()
        .join("clauseo-reuse-paths")
        .join(matter.to_string())
}

/// Records `grants` (each a user and its rights on `matter`'s folder) for `matter` in `ledger`,
/// and marks them applied when `applied`.
fn record(ledger: &mut Ledger, matter: MatterId, grants: &[(Sid, Rights)], applied: bool) {
    let grants = grants
        .iter()
        .map(|(sid, rights)| Grant {
            path: folder(&matter),
            sid: sid.clone(),
            rights: *rights,
            inherit: Inherit::Tree,
            kind: PathKind::Matter,
        })
        .collect();
    let mut plan = AdmittedPlan::from_grants(matter, grants);
    ledger.record_pending(&mut plan).unwrap();
    if applied {
        let ids = plan.entries().to_vec();
        ledger.mark_applied(&plan, &ids).unwrap();
    }
}

/// `matter`'s fence as the launch writes it for `own` in a pool of 4: `Modify` for its own user,
/// a deny of reads and writes for every other.
fn fence(ledger: &mut Ledger, matter: MatterId, own: u32) {
    let mut grants = vec![(user(own), Rights::Modify)];
    grants.extend(
        (1..=4)
            .filter(|n| *n != own)
            .map(|n| (user(n), Rights::DenyReadWrite)),
    );
    record(ledger, matter, &grants, true);
}

const ALLOWS: [Rights; 4] = [
    Rights::Modify,
    Rights::ReadAndExecute,
    Rights::CreateFiles,
    Rights::ReadWriteData,
];
const DENIES: [Rights; 3] = [Rights::DenyWrite, Rights::DenyRead, Rights::DenyReadWrite];

/// The VM defect: once one matter is fenced, its sibling denies name every other pool user, and
/// a second matter must still get one.
#[test]
fn a_matters_sibling_denies_leave_the_other_users_free() {
    let temp = TempDir::new("reuse-siblings");
    let records = TempDir::new("reuse-siblings-ledger");
    let mut ledger = ledger_in(records.path(), &pool(4));
    let mut sids = sids_in(&temp);
    let (one, two, three) = (matter(), matter(), matter());
    let none = OpenSessions::default();
    assert_eq!(
        sids.launch(&one, &pool(4), reread(&ledger), &none),
        Ok(user(1))
    );
    fence(&mut ledger, one, 1);
    assert!(ledger.names_sid(&user(2)) && !ledger.grants_sid(&user(2)));
    assert_eq!(
        sids.launch(&two, &pool(4), reread(&ledger), &none),
        Ok(user(2))
    );
    fence(&mut ledger, two, 2);
    assert_eq!(
        sids.launch(&three, &pool(4), reread(&ledger), &none),
        Ok(user(3))
    );
    // The held users stay held; the fences changed nothing for them.
    let got = sids.launch(&matter(), &pool(3), reread(&ledger), &none);
    let held: Vec<(Sid, Why)> = [(1, one), (2, two), (3, three)]
        .into_iter()
        .map(|(n, m)| (user(n), Why::Held(m)))
        .collect();
    assert_eq!(got, Err(full(3, held)));
}

/// Any allow, pending or applied, holds its user back; a deny of any kind doesn't.
#[test]
fn an_allow_of_any_kind_holds_its_user_back_and_a_deny_does_not() {
    let none = OpenSessions::default();
    for (rights, applied) in ALLOWS.into_iter().flat_map(|r| [(r, false), (r, true)]) {
        let temp = TempDir::new("reuse-allow");
        let records = TempDir::new("reuse-allow-ledger");
        let mut ledger = ledger_in(records.path(), &pool(1));
        record(&mut ledger, matter(), &[(user(1), rights)], applied);
        let state = ledger.entries().next().unwrap().state;
        let want = if applied {
            EntryState::Applied
        } else {
            EntryState::Pending
        };
        assert_eq!(state, want);
        let got = sids_in(&temp).launch(&matter(), &pool(1), reread(&ledger), &none);
        assert_eq!(
            got,
            Err(full(1, vec![(user(1), Why::Entries)])),
            "{rights:?}"
        );
    }
    for rights in DENIES {
        let temp = TempDir::new("reuse-deny");
        let records = TempDir::new("reuse-deny-ledger");
        let mut ledger = ledger_in(records.path(), &pool(1));
        record(&mut ledger, matter(), &[(user(1), rights)], true);
        let got = sids_in(&temp).launch(&matter(), &pool(1), reread(&ledger), &none);
        assert_eq!(got, Ok(user(1)), "{rights:?}");
    }
}

/// A released user is held back until its reset, and only the reset frees it; the mark is saved
/// with the release and survives a reload.
#[test]
fn a_released_user_is_held_back_until_its_reset() {
    let temp = TempDir::new("reuse-reset");
    let records = TempDir::new("reuse-reset-ledger");
    let ledger = ledger_in(records.path(), &pool(2));
    let none = OpenSessions::default();
    let mut sids = sids_in(&temp);
    let (one, two, new) = (matter(), matter(), matter());
    assert_eq!(
        sids.launch(&one, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
    assert_eq!(
        sids.launch(&two, &pool(2), reread(&ledger), &none),
        Ok(user(2))
    );
    assert!(
        sids.resets_due().is_empty(),
        "never-used and held users owe none"
    );
    sids.release(&one, UNIX_EPOCH).unwrap();
    let path = temp.path().join(SANDBOX_DIR).join(MATTERS_FILE);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["version"], 3);
    assert_eq!(json["needs_reset"], serde_json::json!([user(1).as_str()]));
    let reloaded = sids_in(&temp);
    assert_eq!(reloaded.resets_due(), [user(1)]);
    let want = full(
        2,
        vec![(user(1), Why::NeedsReset), (user(2), Why::Held(two))],
    );
    assert_eq!(
        sids.launch(&new, &pool(2), reread(&ledger), &none),
        Err(want)
    );
    // Clearing a user that isn't marked, or one a matter holds, clears nothing.
    assert_eq!(sids.reset_done(&[user(2)]), Ok(vec![]));
    assert_eq!(sids.reset_done(&[user(1), user(2)]), Ok(vec![user(1)]));
    assert!(sids_in(&temp).needs_reset().is_empty());
    assert_eq!(
        sids.launch(&new, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
    // A matter keeping its user needs no reset, marked or not.
    assert_eq!(
        sids.launch(&new, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
}

/// A version 2 file's released users were never reset, so each is read as owed one; a user a
/// row holds isn't, and the next write upgrades the file with the marks.
#[test]
fn a_version_2_files_released_users_owe_a_reset() {
    let temp = TempDir::new("reuse-v2");
    let (a, b, c) = (matter(), matter(), matter());
    let (u1, u2, u3, u4) = (user(1), user(2), user(3), user(4));
    let v2 = format!(
        r#"{{"version":2,"matters":{{
            "{a}":{{"pool_user":"{u1}","released_at":5}},
            "{b}":{{"pool_user":"{u3}","held_before":["{u2}","{u4}"]}},
            "{c}":{{"pool_user":"{u4}"}}}}}}"#
    );
    temp.file(&format!("{SANDBOX_DIR}/{MATTERS_FILE}"), &v2);
    let mut sids = sids_in(&temp);
    let marked: Vec<&Sid> = sids.needs_reset().iter().collect();
    assert_eq!(marked, [&u1, &u2], "user 4 is held by {c}");
    sids.sid_for(&a).unwrap();
    let text = std::fs::read_to_string(temp.path().join(SANDBOX_DIR).join(MATTERS_FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["version"], 3);
    assert_eq!(
        json["needs_reset"],
        serde_json::json!([u1.as_str(), u2.as_str()])
    );
}

/// The `PR4c` reuse row: a forgotten matter's user, reset and given to a new matter, is still
/// denied on every other matter, as their fences name it; reuse removes no deny.
#[test]
fn a_reused_user_stays_denied_on_the_other_matters() {
    let temp = TempDir::new("reuse-row");
    let records = TempDir::new("reuse-row-ledger");
    let mut ledger = ledger_in(records.path(), &pool(4));
    let mut sids = sids_in(&temp);
    let (old, kept, new) = (matter(), matter(), matter());
    let none = OpenSessions::default();
    assert_eq!(
        sids.launch(&old, &pool(4), reread(&ledger), &none),
        Ok(user(1))
    );
    assert_eq!(
        sids.launch(&kept, &pool(4), reread(&ledger), &none),
        Ok(user(2))
    );
    // `kept`'s fence denies user 1; `old` was forgotten, so its own entries are gone.
    fence(&mut ledger, kept, 2);
    sids.release(&old, UNIX_EPOCH).unwrap();
    sids.reset_done(&[user(1)]).unwrap();
    assert_eq!(
        sids.launch(&new, &pool(1), reread(&ledger), &none),
        Ok(user(1))
    );
    let again = Ledger::open(ledger.path(), &pool(4), &Reserved::default()).unwrap();
    let denied = again.matter_entries(kept).into_iter().any(|entry| {
        entry.sid == user(1) && entry.rights == Rights::DenyReadWrite && entry.path == folder(&kept)
    });
    assert!(denied, "user 1 is still denied on {kept}");
}

/// Version 3 always writes `needs_reset`, empty or not, and refuses a file without it.
#[test]
fn version_3_always_writes_needs_reset_and_requires_it() {
    let temp = TempDir::new("reuse-v3-field");
    let mut sids = sids_in(&temp);
    sids.sid_for(&matter()).unwrap();
    let path = temp.path().join(SANDBOX_DIR).join(MATTERS_FILE);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["needs_reset"], serde_json::json!([]));
    temp.file(
        &format!("{SANDBOX_DIR}/{MATTERS_FILE}"),
        r#"{"version":3,"matters":{}}"#,
    );
    assert!(
        MatterSids::load(temp.path()).is_err(),
        "a missing field fails closed"
    );
}

/// A release that lands while set-up clears its marks (another instance, between set-up's read
/// and its clear) keeps its user marked: the clear reads the file again under the lock.
#[test]
fn a_release_during_a_clear_stays_marked() {
    let temp = TempDir::new("reuse-clear-race");
    let records = TempDir::new("reuse-clear-race-ledger");
    let ledger = ledger_in(records.path(), &pool(2));
    let none = OpenSessions::default();
    let mut app = sids_in(&temp);
    let (one, two) = (matter(), matter());
    assert_eq!(
        app.launch(&one, &pool(2), reread(&ledger), &none),
        Ok(user(1))
    );
    assert_eq!(
        app.launch(&two, &pool(2), reread(&ledger), &none),
        Ok(user(2))
    );
    app.release(&one, UNIX_EPOCH).unwrap();
    // Set-up reads the marks, then the app releases the other matter before set-up clears.
    let mut setup = sids_in(&temp);
    assert_eq!(setup.resets_due(), [user(1)]);
    app.release(&two, UNIX_EPOCH).unwrap();
    assert_eq!(setup.reset_done(&setup.resets_due()), Ok(vec![user(1)]));
    let after: Vec<Sid> = sids_in(&temp).needs_reset().iter().cloned().collect();
    assert_eq!(after, [user(2)], "the later release keeps its mark");
}

/// The set-up's top-up counts free exactly what `assign` would hand out once the run's resets
/// are done: a user held only by a hand-back owed isn't free, a marked user no matter holds is,
/// and a ledger that can't be read holds every user.
#[test]
fn the_top_ups_count_holds_back_what_assign_does() {
    let temp = TempDir::new("reuse-topup-count");
    let records = TempDir::new("reuse-topup-ledger");
    let ledger = ledger_owing(&records, &matter().to_string(), &user(2));
    let none = OpenSessions::default();
    let mut sids = sids_in(&temp);
    let (one, new) = (matter(), matter());
    assert_eq!(
        sids.launch(&one, &pool(4), reread(&ledger), &none),
        Ok(user(1))
    );
    sids.release(&one, UNIX_EPOCH).unwrap();
    // `assign` holds back user 1 (owed a reset) and user 2 (a hand-back owed) now.
    assert_eq!(
        sids.launch(&new, &pool(4), reread(&ledger), &none),
        Ok(user(3))
    );
    sids.release(&new, UNIX_EPOCH).unwrap();
    // After the run's resets, users 1 and 3 are reset and free; user 2 still waits on its
    // hand-back.
    let held = sids.held_after_resets(&pool(4), reread(&ledger)());
    assert_eq!(held, [user(2)]);
    let unreadable = io::Error::other("unreadable");
    assert_eq!(sids.held_after_resets(&pool(4), Err(unreadable)), pool(4));
}
