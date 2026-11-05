use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use super::*;
use crate::test_support::TempDir;
use crate::windows::acl_plan::LOW_LABEL_SID;
use crate::windows::test_world::{
    GROUP_SID, MATTER_SID, OTHER_POOL_SIDS, USER_SID, matter_one, matter_two, reserved, trusted,
};
use crate::windows::write::write_atomic_with;

/// `name` as an absolute path on every OS.
fn abs(name: &str) -> PathBuf {
    std::env::temp_dir().join("clauseo-ledger-paths").join(name)
}

fn grant(path: &Path, sid: &str, rights: Rights) -> Grant {
    Grant {
        path: path.to_owned(),
        sid: Sid::parse(sid).unwrap(),
        rights,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    }
}

/// A plan for `matter`: a pair on one folder, and a deny.
fn plan_for(matter: MatterId, folder: &str) -> AdmittedPlan {
    let folder = abs(folder);
    AdmittedPlan::from_grants(
        matter,
        vec![
            grant(&folder, USER_SID, Rights::Modify),
            grant(&folder, MATTER_SID, Rights::Modify),
            grant(&folder.join(".git"), MATTER_SID, Rights::DenyWrite),
        ],
    )
}

fn open(path: &Path) -> Ledger {
    Ledger::open(path, &trusted(), &reserved()).unwrap()
}

fn states(ledger: &Ledger) -> Vec<(u64, EntryState)> {
    ledger.entries().map(|e| (e.id.0, e.state)).collect()
}

fn ids(ledger: &Ledger) -> Vec<EntryId> {
    ledger.entries().map(|e| e.id).collect()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn the_ledger_lives_in_the_data_folders_sandbox_folder() {
    let path = ledger_path(Path::new("/data"));
    assert_eq!(
        path,
        Path::new("/data").join("sandbox").join("acl-ledger.json")
    );
}

#[test]
fn a_missing_ledger_is_empty_and_stays_unwritten() {
    let temp = TempDir::new("ledger-missing");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    assert_eq!(ledger.entries().count(), 0);
    assert_eq!(
        ledger.reconcile(|_| Ok(true)).unwrap(),
        Reconciled::default()
    );
    let forgotten = ledger.forget_matter(matter_one(), |_| Ok(())).unwrap();
    assert_eq!(forgotten, Forgotten::default());
    assert!(!temp.path().join("sandbox").exists());
}

#[test]
fn entries_are_pending_then_applied() {
    let temp = TempDir::new("ledger-pending-applied");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    assert_eq!(plan.entries(), [EntryId(1), EntryId(2), EntryId(3)]);
    assert_eq!(plan.created(), plan.entries());
    // Written before any permission entry would be: the file already lists them, pending.
    let pending = [
        (1, EntryState::Pending),
        (2, EntryState::Pending),
        (3, EntryState::Pending),
    ];
    assert_eq!(states(&open(&path)), pending);
    assert!(
        String::from_utf8(read(&path))
            .unwrap()
            .contains("\"pending\"")
    );
    let first_two = plan.entries()[..2].to_vec();
    ledger.mark_applied(&plan, &first_two).unwrap();
    let reopened = open(&path);
    let applied = [
        (1, EntryState::Applied),
        (2, EntryState::Applied),
        (3, EntryState::Pending),
    ];
    assert_eq!(states(&reopened), applied);
    let entry = reopened.entries().next().unwrap();
    assert_eq!(entry.matter, matter_one());
    assert_eq!(entry.grant(), plan.grants()[0]);
    assert!(temp.path().join("sandbox").join(LEDGER_LOCK).exists());
}

#[test]
fn a_plan_is_recorded_once() {
    let temp = TempDir::new("ledger-recorded-once");
    let mut ledger = open(&ledger_path(temp.path()));
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let error = ledger.record_pending(&mut plan).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn a_grant_listed_already_keeps_its_entry_and_the_file() {
    let temp = TempDir::new("ledger-relisted");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut first = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut first).unwrap();
    let all = first.entries().to_vec();
    ledger.mark_applied(&first, &all).unwrap();
    let before = read(&path);
    let mut again = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut again).unwrap();
    assert_eq!(again.entries(), first.entries());
    assert_eq!(again.created(), [] as [EntryId; 0]);
    assert_eq!(read(&path), before);
    // The same grants for another matter are that matter's own entries.
    let mut other = plan_for(matter_two(), "m1");
    ledger.record_pending(&mut other).unwrap();
    assert_eq!(other.created(), [EntryId(4), EntryId(5), EntryId(6)]);
}

#[test]
fn two_ledgers_on_one_file_keep_each_others_entries() {
    // Two processes (two hosts) open the ledger before either writes; each records under the
    // lock after reading the file again, so neither loses the other's entries.
    let temp = TempDir::new("ledger-two-processes");
    let path = ledger_path(temp.path());
    let (mut a, mut b) = (open(&path), open(&path));
    let mut plan_a = plan_for(matter_one(), "a");
    let mut plan_b = plan_for(matter_two(), "b");
    a.record_pending(&mut plan_a).unwrap();
    b.record_pending(&mut plan_b).unwrap();
    let reloaded = open(&path);
    assert_eq!(reloaded.entries().count(), 6);
    let mut all: Vec<EntryId> = plan_a
        .entries()
        .iter()
        .chain(plan_b.entries())
        .copied()
        .collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 6, "two launches' entry numbers don't collide");
    // And a, whose cache is stale, still writes over both.
    let applied = plan_a.entries().to_vec();
    a.mark_applied(&plan_a, &applied).unwrap();
    assert_eq!(open(&path).entries().count(), 6);
}

#[test]
fn a_rollback_drops_only_its_own_plans_entries() {
    let temp = TempDir::new("ledger-rollback");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut earlier = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut earlier).unwrap();
    // This launch lists one grant the earlier launch had, and two new ones.
    let mut grants = earlier.grants()[..1].to_vec();
    grants.push(grant(&abs("m1/notes"), USER_SID, Rights::Modify));
    grants.push(grant(&abs("m1/notes"), MATTER_SID, Rights::Modify));
    let mut this = AdmittedPlan::from_grants(matter_one(), grants);
    ledger.record_pending(&mut this).unwrap();
    assert_eq!(this.created(), [EntryId(4), EntryId(5)]);
    // Rolling back everything this plan touched drops only what it added; ids of the earlier
    // launch, shared or not, stay.
    let everything: Vec<EntryId> = (1..=5).map(EntryId).collect();
    ledger.drop_entries(&mut this, &everything).unwrap();
    assert_eq!(ids(&open(&path)), [EntryId(1), EntryId(2), EntryId(3)]);
    assert_eq!(this.created(), [] as [EntryId; 0]);
    // A plan can't mark another plan's entries either.
    ledger.mark_applied(&this, &[EntryId(2)]).unwrap();
    assert_eq!(states(&open(&path))[1], (2, EntryState::Pending));
}

#[test]
fn reconcile_keeps_a_pending_entry_whose_ace_is_there_and_drops_one_whose_isnt() {
    let temp = TempDir::new("ledger-reconcile");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut done_plan = AdmittedPlan::from_grants(
        matter_one(),
        vec![grant(&abs("m0"), USER_SID, Rights::Modify)],
    );
    ledger.record_pending(&mut done_plan).unwrap();
    ledger.mark_applied(&done_plan, &[EntryId(1)]).unwrap();
    // A crash after the pending entries: 2 has its ACE, 3 doesn't, 4 can't be checked.
    let mut crashed = AdmittedPlan::from_grants(
        matter_one(),
        vec![
            grant(&abs("m1"), USER_SID, Rights::Modify),
            grant(&abs("m1"), MATTER_SID, Rights::Modify),
            grant(&abs("m1/x"), MATTER_SID, Rights::DenyWrite),
        ],
    );
    ledger.record_pending(&mut crashed).unwrap();
    let mut asked = Vec::new();
    let done = ledger
        .reconcile(|entry| {
            asked.push(entry.id.0);
            match entry.id.0 {
                2 => Ok(true),
                3 => Ok(false),
                _ => Err(io::Error::other("the folder is held")),
            }
        })
        .unwrap();
    assert_eq!(asked, [2, 3, 4], "an applied entry isn't checked");
    assert_eq!(
        done,
        Reconciled {
            applied: 1,
            dropped: 1,
            unchecked: 1
        }
    );
    let after = [
        (1, EntryState::Applied),
        (2, EntryState::Applied),
        (4, EntryState::Pending),
    ];
    assert_eq!(states(&open(&path)), after);
    // Numbers are never reused, so a dropped entry's number can't name a new one.
    let mut next = plan_for(matter_two(), "m9");
    ledger.record_pending(&mut next).unwrap();
    assert_eq!(next.entries()[0], EntryId(5));
}

#[test]
fn marking_an_entry_another_process_dropped_fails() {
    let temp = TempDir::new("ledger-mark-dropped");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    // Another host starts, finds no ACE yet, and drops the pending entries.
    let done = open(&path).reconcile(|_| Ok(false)).unwrap();
    assert_eq!(done.dropped, 3);
    let all = plan.entries().to_vec();
    let error = ledger.mark_applied(&plan, &all).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    let missing = error
        .get_ref()
        .and_then(|e| e.downcast_ref::<MissingEntries>());
    assert_eq!(missing, Some(&MissingEntries(all)));
    assert_eq!(
        open(&path).entries().count(),
        0,
        "nothing was marked or put back"
    );
}

#[test]
fn labels_and_the_pool_denies_are_recorded_and_forget_matter_returns_them() {
    // Each new kind of item: the Low label, another pool user's deny, the group's sibling deny.
    let temp = TempDir::new("ledger-new-kinds");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let matter = abs("m1");
    let item = |sid: &str, rights, kind| Grant {
        path: matter.clone(),
        sid: Sid::parse(sid).unwrap(),
        rights,
        inherit: Inherit::Tree,
        kind,
    };
    let grants = vec![
        item(LOW_LABEL_SID, Rights::LowLabel, PathKind::Matter),
        item(OTHER_POOL_SIDS[0], Rights::DenyReadWrite, PathKind::Matter),
        Grant {
            path: abs("Drafts"),
            ..item(GROUP_SID, Rights::DenyRead, PathKind::Sibling)
        },
    ];
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants.clone());
    ledger.record_pending(&mut plan).unwrap();
    let reopened = open(&path);
    let read: Vec<Grant> = reopened.entries().map(Entry::grant).collect();
    assert_eq!(read, grants, "every kind is read back and trusted");
    let mut handed = Vec::new();
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            handed.push(entry.grant());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 3,
            kept: 0
        }
    );
    assert_eq!(handed, grants);
}

#[test]
fn a_label_naming_anything_but_the_low_level_is_set_aside() {
    let temp = TempDir::new("ledger-label-trust");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let label = |sid: &str| Grant {
        path: abs("m1"),
        sid: Sid::parse(sid).unwrap(),
        rights: Rights::LowLabel,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![label(USER_SID)]);
    ledger.record_pending(&mut plan).unwrap();
    let reopened = open(&path);
    assert_eq!(reopened.entries().count(), 0);
    assert_eq!(reopened.set_aside_entries(), 1);
}

#[test]
fn reconcile_with_nothing_to_change_leaves_the_file() {
    let temp = TempDir::new("ledger-reconcile-nothing");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let all = plan.entries().to_vec();
    ledger.mark_applied(&plan, &all).unwrap();
    let before = read(&path);
    let done = ledger
        .reconcile(|_| panic!("no pending entry to check"))
        .unwrap();
    assert_eq!(done, Reconciled::default());
    assert_eq!(read(&path), before);
}

#[test]
fn forget_matter_drops_exactly_that_matters_entries() {
    let temp = TempDir::new("ledger-forget");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut one = plan_for(matter_one(), "m1");
    let mut two = plan_for(matter_two(), "m2");
    ledger.record_pending(&mut one).unwrap();
    ledger.record_pending(&mut two).unwrap();
    let listed: Vec<EntryId> = ledger
        .matter_entries(matter_one())
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(listed, one.entries());
    let mut removed = Vec::new();
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            removed.push(entry.id);
            Ok(())
        })
        .unwrap();
    assert_eq!(removed, one.entries());
    assert_eq!(
        done,
        Forgotten {
            removed: 3,
            kept: 0
        }
    );
    let reopened = open(&path);
    assert_eq!(ids(&reopened), two.entries());
    assert!(reopened.entries().all(|e| e.matter == matter_two()));
}

#[test]
fn forget_matter_keeps_an_entry_whose_removal_failed() {
    let temp = TempDir::new("ledger-forget-retry");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let failing = plan.entries()[1];
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            if entry.id == failing {
                Err(io::Error::other("WRITE_DAC was taken away"))
            } else {
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 2,
            kept: 1
        }
    );
    assert_eq!(ids(&open(&path)), [failing]);
}

#[test]
fn a_failed_replace_leaves_the_old_file_whole_and_no_temporary_file() {
    let temp = TempDir::new("ledger-atomic");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let before = read(&path);
    let error = write_atomic_with(&path, b"{\"version\": 1, \"entries\": [", |file, bytes| {
        file.write_all(&bytes[..10])?;
        Err(io::Error::other("the disk is full"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "the disk is full");
    assert_eq!(read(&path), before);
    let mut names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [LEDGER_FILE, LEDGER_LOCK]);
    let applied = plan.entries().to_vec();
    ledger.mark_applied(&plan, &applied).unwrap();
    assert_eq!(open(&path).entries().count(), 3);
}

#[test]
fn a_file_where_the_folder_goes_is_refused() {
    let temp = TempDir::new("ledger-folder-is-file");
    let mut ledger = open(&ledger_path(temp.path()));
    temp.file("sandbox", "not a folder");
    let mut plan = plan_for(matter_one(), "m1");
    assert!(ledger.record_pending(&mut plan).is_err());
    assert_eq!(
        ledger.entries().count(),
        0,
        "memory matches the file after a failed write"
    );
    assert_eq!(plan.entries(), [] as [EntryId; 0]);
}

/// On Windows the ledger's folder is made private (PR2's `make_sandbox_dir`): owned by this
/// user, with the protected DACL.
#[cfg(windows)]
#[test]
fn the_ledgers_folder_is_made_private_and_owned_by_the_user() {
    let temp = TempDir::new("ledger-private-folder");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let folder = path.parent().unwrap();
    let owner = crate::windows::private_dir::owner(folder).unwrap();
    let me = crate::windows::record::current_user_sid().unwrap();
    assert!(
        crate::windows::private_dir::owner_allowed(&owner, &me),
        "{owner}"
    );
}

/// What is read back from a file: unread, untrusted and refused files.
mod file;
