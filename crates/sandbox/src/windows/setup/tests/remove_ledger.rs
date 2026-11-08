//! `--remove`'s ledger step against the set-up's fake. As the owner, every matter is forgotten
//! while acting as the owner, and anything left keeps the record. As an administrator for an
//! absent owner (per-machine, LOCAL SYSTEM under Intune), a crafted ledger can make it remove
//! one access entry for a recorded pool SID inside a recorded matter folder that passes the path
//! check, and take the Low label off such a folder's root, and nothing else: no label written,
//! no walk, no hand-back, no add, no ledger write.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::fake::{OTHER_OWNER, OWNER};
use super::remove::{assert_all_gone, pool};
use super::remove_fake::Remover;
use super::*;
use crate::windows::SET_ASIDE_DIR;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit, LOW_LABEL_SID};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Ledger, RecordedFile, ledger_path};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::sids::Reserved;
use crate::windows::write;
use crate::windows::writer::FileId;

/// What every launch leaves in a matter's set-aside folder: the record and the lock.
pub(super) const LEFT_BY_LAUNCH: [&str; 2] = ["record.json", "set-aside.lock"];

fn matter(id: &str) -> MatterId {
    id.parse().unwrap()
}

/// The owner's matter.
pub(super) fn matter_one() -> MatterId {
    matter("01890a5d-ac96-774b-bcce-b302099a8057")
}

/// A matter a crafted ledger made up.
fn matter_two() -> MatterId {
    matter("01890a5d-ac96-774b-8cce-b302099a8058")
}

/// The fake's account or group with relative id `rid`.
fn pool_sid(rid: u32) -> Sid {
    super::fake::sid(rid)
}

fn grant(path: &Path, sid: &Sid, rights: Rights, inherit: Inherit, kind: PathKind) -> Grant {
    Grant {
        path: path.to_owned(),
        sid: sid.clone(),
        rights,
        inherit,
        kind,
    }
}

/// Every SID the tests' ledgers name: the pool users, the group and the owner.
fn named() -> Vec<Sid> {
    (1000..=1004).map(pool_sid).chain([owner()]).collect()
}

/// Records `grants` for `matter` in the ledger in `data_dir`, pending, as a launch that
/// stopped leaves them (a removal takes back every state); with `file`, the grant at that
/// index records a file by ID, as a credential link's does.
fn record(data_dir: &Path, matter: MatterId, grants: Vec<Grant>, file: Option<usize>) {
    let mut ledger = Ledger::open(ledger_path(data_dir), &named(), &Reserved::new([])).unwrap();
    let mut plan = AdmittedPlan::from_grants(matter, grants);
    ledger.record_pending(&mut plan).unwrap();
    if let Some(at) = file {
        let id = plan.entries()[at];
        let recorded = RecordedFile::new(
            data_dir.join("credentials.json"),
            FileId {
                volume: 7,
                id: [7; 16],
            },
        );
        ledger.set_file(&plan, id, &recorded).unwrap();
    }
}

/// The owner's matter folder, under the fake's temporary folder.
fn matters(machine: &Remover) -> PathBuf {
    machine.m.dir.path().join("matters")
}

/// One matter of the owner's: the pool user's allow on its folder, another's deny in it.
pub(super) fn owners_matter(machine: &Remover) -> PathBuf {
    let m1 = matters(machine).join("m1");
    let grants = vec![
        grant(
            &m1,
            &pool_sid(1001),
            Rights::Modify,
            Inherit::Tree,
            PathKind::Matter,
        ),
        grant(
            &m1.join(".git"),
            &pool_sid(1002),
            Rights::DenyWrite,
            Inherit::Tree,
            PathKind::ReadOnlyInMatter,
        ),
    ];
    record(&machine.m.data_dir(), matter_one(), grants, None);
    m1
}

#[test]
fn as_the_owner_every_matter_is_forgotten_as_the_owner_then_the_record_goes() {
    let (mut machine, mut acl) = pool("remove-ledger-owner", Some(owner()));
    let m1 = owners_matter(&machine);
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(
        report.ledger,
        LedgerWork::AsOwner {
            matters: 1,
            removed: 2
        }
    );
    // Both entries came off acting as the owner; the shared denies as this process.
    let as_owner: Vec<&str> = acl
        .removes
        .iter()
        .filter(|(_, acting)| *acting)
        .map(|(line, _)| line.as_str())
        .collect();
    assert_eq!(as_owner, ["matters/m1 1001", "matters/m1/.git 1002"]);
    // Two shared denies per pool user's LocalLow, and DeviceSync's.
    let shared = 2 * MIN_FREE_AFTER_RUN + 1;
    assert_eq!(
        acl.removes.iter().filter(|(_, acting)| !acting).count(),
        shared
    );
    let ledger = Machine::ledger(&mut machine.m, &named()).unwrap();
    assert_eq!(ledger.entries().count(), 0);
    assert_eq!(ledger.set_aside_entries(), 0);
    assert!(m1.starts_with(machine.m.dir.path()));
    assert_all_gone(&machine, &acl);
}

/// A set-aside record with no open session, as a clean close leaves it.
pub(super) const CLOSED: &[u8] =
    b"{\"version\": 1, \"matter\": \"01890a5d-ac96-774b-bcce-b302099a8057\", \
    \"sessions\": [], \"names\": []}";

/// `<data_dir>\sandbox\set-aside\<matter>`, made, holding what every launch leaves there.
pub(super) fn bookkeeping(data_dir: &Path, matter: MatterId) -> PathBuf {
    let set_aside = data_dir.join(SANDBOX_DIR).join(SET_ASIDE_DIR);
    write::make_sandbox_dir(&set_aside).unwrap();
    let folder = set_aside.join(matter.to_string());
    write::make_sandbox_dir(&folder).unwrap();
    for name in LEFT_BY_LAUNCH {
        let bytes: &[u8] = if name == "record.json" { CLOSED } else { b"" };
        write::write_atomic(&folder.join(name), bytes).unwrap();
    }
    folder
}

/// A pass folder in the matter's set-aside `folder` holding a moved `.git\HEAD`, and its
/// manifest: the lawyer's files.
fn moved_files(folder: &Path) -> PathBuf {
    let git = folder.join("20261107T101500-000Z").join(".git");
    write::make_sandbox_dir(&git).unwrap();
    write::write_atomic(&git.join("HEAD"), b"ref: refs/heads/main").unwrap();
    write::write_atomic(&folder.join("manifest.json"), b"{}").unwrap();
    git.join("HEAD")
}

#[test]
fn as_the_owner_a_forgotten_matters_bookkeeping_goes_unless_moved_files_are_beside_it() {
    let (mut machine, mut acl) = pool("remove-ledger-set-aside", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let m2 = matters(&machine).join("m2");
    let allow = grant(
        &m2,
        &pool_sid(1003),
        Rights::Modify,
        Inherit::Tree,
        PathKind::Matter,
    );
    record(&data_dir, matter_two(), vec![allow], None);
    // Bookkeeping only: it goes, and the matter's folder with it.
    let one = bookkeeping(&data_dir, matter_one());
    // Bookkeeping beside a pass's moved files: everything stays.
    let two = bookkeeping(&data_dir, matter_two());
    let head = moved_files(&two);
    // A matter the ledger doesn't name was never forgotten: its bookkeeping stays.
    let unnamed = matter("01890a5d-ac96-774b-8cce-b302099a8059");
    let three = bookkeeping(&data_dir, unnamed);

    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_eq!(machine.forgot, [matter_one(), matter_two()]);
    assert!(
        std::fs::symlink_metadata(&one).is_err(),
        "{}",
        one.display()
    );
    // All or nothing: beside moved files, the bookkeeping stays too (the set-aside is Kept).
    for name in LEFT_BY_LAUNCH {
        assert!(two.join(name).exists(), "{name}");
        assert!(three.join(name).exists(), "{name}");
    }
    assert_eq!(std::fs::read(&head).unwrap(), b"ref: refs/heads/main");
    assert!(two.join("manifest.json").exists());
    assert_all_gone(&machine, &acl);
}

#[test]
fn as_an_administrator_no_set_aside_bookkeeping_is_ever_deleted() {
    // The owner absent (another administrator, or LOCAL SYSTEM): K1, nothing deleted in the
    // owner's folders. The fake also asserts the call is made only acting as the owner.
    let admin = Sid::parse(OTHER_OWNER).unwrap();
    for invoker in [Some(admin), None] {
        let (mut machine, mut acl) = pool("remove-ledger-set-aside-admin", invoker);
        let data_dir = machine.m.data_dir();
        let m1 = owners_matter(&machine);
        machine.admin_owned = vec![(m1, owner())];
        let one = bookkeeping(&data_dir, matter_one());
        let removed = remove(&mut machine, &mut acl, true).unwrap();
        assert!(matches!(removed, Removed::All(_)), "{removed:?}");
        assert_eq!(machine.forgot, Vec::<MatterId>::new());
        for name in LEFT_BY_LAUNCH {
            assert!(one.join(name).exists(), "{name}");
        }
    }
}

#[test]
fn as_the_owner_a_bookkeeping_name_that_is_a_folder_is_left_and_the_removal_still_finishes() {
    let (mut machine, mut acl) = pool("remove-ledger-set-aside-folder", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let one = bookkeeping(&data_dir, matter_one());
    let [record_name, _] = LEFT_BY_LAUNCH;
    write::remove_owned_file(&one.join(record_name)).unwrap();
    write::make_sandbox_dir(&one.join(record_name)).unwrap();
    write::write_atomic(&one.join(record_name).join("kept.txt"), b"x").unwrap();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert!(one.join(record_name).join("kept.txt").exists());
    assert_all_gone(&machine, &acl);
}

#[test]
fn as_the_owner_a_removal_that_fails_keeps_the_ledger_the_group_and_the_record() {
    let (mut machine, mut acl) = pool("remove-ledger-fails", Some(owner()));
    owners_matter(&machine);
    // The first matter entry's removal fails.
    let mut probe = pool("remove-ledger-probe", Some(owner()));
    owners_matter(&probe.0);
    remove(&mut probe.0, &mut probe.1, false).unwrap();
    let at = probe
        .0
        .lines()
        .iter()
        .position(|l| l == "remove matters/m1 1001")
        .unwrap();
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Ledger, SetupExit::RemovedPartly)
    );
    let record = machine.m.state.record.clone().unwrap();
    assert!(record.removing);
    assert_eq!(
        record.pool.len(),
        MIN_FREE_AFTER_RUN,
        "the ledger's pool SIDs stay trusted"
    );
    assert!(machine.m.state.group.is_some());
    let ledger = Machine::ledger(&mut machine.m, &record.pool).unwrap();
    assert!(ledger.entries().count() > 0);
    // Reinstall and uninstall again: the next run finishes.
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_all_gone(&machine, &acl);
}

#[test]
fn as_the_owner_a_data_folder_that_fails_its_check_keeps_everything_after_the_accounts() {
    let (mut machine, mut acl) = pool("remove-ledger-dir", Some(owner()));
    owners_matter(&machine);
    machine.data_dir_bad = true;
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.step, RemoveStep::Ledger);
    assert!(machine.m.state.record.is_some());
    assert!(machine.m.state.group.is_some());
    assert!(
        !acl.removes
            .iter()
            .any(|(line, _)| line.starts_with("matters/"))
    );
}

#[test]
fn as_an_administrator_a_crafted_ledger_is_refused_outside_the_recorded_matters() {
    let admin = Sid::parse(OTHER_OWNER).unwrap();
    for invoker in [Some(admin), None] {
        let (mut machine, mut acl) = pool("remove-ledger-crafted", invoker);
        let data_dir = machine.m.data_dir();
        let m1 = matters(&machine).join("m1");
        let elsewhere = machine.m.dir.path().join("elsewhere");
        let system = machine.m.dir.path().join("Windows");
        let root = m1.ancestors().last().unwrap().to_owned();
        let owner_sid = owner();
        let group = pool_sid(1000);
        let low = Sid::parse(LOW_LABEL_SID).unwrap();
        let g =
            |path: &Path, sid: &Sid, rights, kind| grant(path, sid, rights, Inherit::Tree, kind);
        let one = vec![
            // Removed: the matter folder's allow, and a deny in it, both passing the check.
            g(&m1, &pool_sid(1001), Rights::Modify, PathKind::Matter),
            g(
                &m1.join(".git"),
                &pool_sid(1002),
                Rights::DenyWrite,
                PathKind::ReadOnlyInMatter,
            ),
            // In the matter, but its folder fails the path check (a link, say): left.
            g(
                &m1.join("linked"),
                &group,
                Rights::DenyRead,
                PathKind::Sibling,
            ),
            // The Low label on a matter folder that passes the check: taken off, never put back.
            g(&m1, &low, Rights::LowLabel, PathKind::Matter),
            // Outside every recorded matter: never even checked.
            g(
                &elsewhere,
                &pool_sid(1002),
                Rights::DenyRead,
                PathKind::OtherMatter,
            ),
            // A credential link's entry records a file by ID: no path to check, left.
            g(
                &m1.join("link"),
                &pool_sid(1001),
                Rights::ReadWriteData,
                PathKind::CredentialLink,
            ),
            // The owner's own SID: the ledger doesn't trust it, so it is never listed.
            g(&m1, &owner_sid, Rights::Modify, PathKind::Matter),
            // A Low label below a matter folder's root: left (only roots lose theirs).
            g(
                &m1.join(".git"),
                &low,
                Rights::LowLabel,
                PathKind::ReadOnlyInMatter,
            ),
        ];
        record(&data_dir, matter_one(), one, Some(5));
        let two = vec![
            // A matter made up over a folder the owner doesn't own: checked, refused, left.
            g(&system, &pool_sid(1003), Rights::Modify, PathKind::Matter),
            // Its Low label: the folder fails the check, so the label stays.
            g(&system, &low, Rights::LowLabel, PathKind::Matter),
            g(
                &system.join("System32"),
                &pool_sid(1003),
                Rights::DenyWrite,
                PathKind::Matter,
            ),
            // A drive's root as a matter: never a matter folder, never checked.
            g(&root, &pool_sid(1004), Rights::Modify, PathKind::Matter),
        ];
        record(&data_dir, matter_two(), two, None);
        machine.admin_owned = vec![(m1.clone(), owner()), (m1.join(".git"), owner())];
        let bytes = std::fs::read(ledger_path(&data_dir)).unwrap();

        let removed = remove(&mut machine, &mut acl, true).unwrap();
        let Removed::All(report) = removed else {
            panic!("{removed:?}");
        };
        // Eleven trusted entries; two access entries and one root's label removed.
        assert_eq!(
            report.ledger,
            LedgerWork::AsAdmin {
                removed: 3,
                left: 8
            }
        );
        let lines = machine.lines();
        let checked: Vec<&str> = lines
            .iter()
            .filter_map(|l| l.strip_prefix("admin_target "))
            .collect();
        assert_eq!(
            checked,
            [
                "matters/m1",
                "matters/m1/.git",
                "matters/m1/linked",
                "Windows",
                "Windows/System32"
            ]
        );
        let ledger_lines: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("matters/") || l.contains("Windows") || l.contains("elsewhere"))
            .filter(|l| !l.starts_with("admin_target "))
            .collect();
        assert_eq!(
            ledger_lines,
            [
                "remove matters/m1 1001",
                "remove matters/m1/.git 1002",
                "remove matters/m1 4096"
            ],
            "only removals: the access entries, then the passing root's label; no walk, no label \
             written, no hand-back, no add"
        );
        for kind in ["add ", "restore_label ", "reclaim ", "remove_recorded "] {
            assert!(!lines.iter().any(|l| l.starts_with(kind)), "{lines:?}");
        }
        // Nothing ran as the owner, and the ledger is as it was, byte for byte.
        assert!(!machine.acted);
        assert!(acl.removes.iter().all(|(_, acting)| !acting));
        assert_eq!(std::fs::read(ledger_path(&data_dir)).unwrap(), bytes);
        // The record goes anyway: the entries left name dead SIDs (D-B9 as ruled, K10).
        assert_eq!(machine.m.state.record, None);
        assert_eq!(machine.m.state.group, None);
    }
}

#[test]
fn as_an_administrator_a_data_folder_that_isnt_the_owners_is_never_read() {
    let (mut machine, mut acl) = pool("remove-ledger-admin-dir", None);
    owners_matter(&machine);
    machine.data_dir_bad = true;
    let removed = remove(&mut machine, &mut acl, true).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert!(
        matches!(report.ledger, LedgerWork::Skipped(_)),
        "{report:?}"
    );
    assert!(!machine.lines().iter().any(|l| l.contains("matters/")));
    assert_eq!(machine.m.state.record, None);
}

#[test]
fn as_an_administrator_a_matter_folder_administrators_or_system_owns_loses_only_our_entries() {
    // The built-in Administrator, or UAC off: the lawyer's matter folders are Administrators'.
    for found in ["S-1-5-32-544", "S-1-5-18"] {
        let (mut machine, mut acl) = pool("remove-ledger-admin-owned", None);
        let data_dir = machine.m.data_dir();
        let m1 = matters(&machine).join("m1");
        let low = Sid::parse(LOW_LABEL_SID).unwrap();
        let g = |sid: &Sid, rights| grant(&m1, sid, rights, Inherit::Tree, PathKind::Matter);
        let grants = vec![
            g(&pool_sid(1001), Rights::Modify),
            g(&low, Rights::LowLabel),
        ];
        record(&data_dir, matter_one(), grants, None);
        machine.admin_owned = vec![(m1.clone(), Sid::parse(found).unwrap())];
        let removed = remove(&mut machine, &mut acl, true).unwrap();
        let Removed::All(report) = removed else {
            panic!("{removed:?}");
        };
        let work = LedgerWork::AsAdmin {
            removed: 2,
            left: 0,
        };
        assert_eq!(report.ledger, work, "{found}");
        let lines = machine.lines();
        let on_matter: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("matters/") && !l.starts_with("admin_target "))
            .collect();
        assert_eq!(
            on_matter,
            ["remove matters/m1 1001", "remove matters/m1 4096"],
            "{found}: our allow, then our Low label; nothing else"
        );
        for kind in ["add ", "restore_label ", "reclaim ", "remove_recorded "] {
            assert!(!lines.iter().any(|l| l.starts_with(kind)), "{lines:?}");
        }
    }
}

#[test]
fn as_an_administrator_a_matter_folder_another_user_or_a_pool_user_owns_is_refused() {
    let other_user = "S-1-5-21-2000000001-2000000002-2000000003-1001".to_owned();
    for found in [other_user, pool_sid(1001).as_str().to_owned()] {
        let (mut machine, mut acl) = pool("remove-ledger-other-owned", None);
        let m1 = owners_matter(&machine);
        machine.admin_owned = vec![(m1, Sid::parse(&found).unwrap())];
        let removed = remove(&mut machine, &mut acl, true).unwrap();
        let Removed::All(report) = removed else {
            panic!("{removed:?}");
        };
        let work = LedgerWork::AsAdmin {
            removed: 0,
            left: 2,
        };
        assert_eq!(report.ledger, work, "{found}");
        let lines = machine.lines();
        assert!(
            !lines.iter().any(|l| l.starts_with("remove matters/")),
            "{found}: {lines:?}"
        );
    }
}

/// The removal's ledger work as an administrator for an absent owner, with the data folder and
/// its `sandbox` folder owned by `found`, and the matter folder by the owner.
fn admins_ledger_work(test: &str, found: [&str; 2]) -> LedgerWork {
    let (mut machine, mut acl) = pool(test, None);
    let m1 = owners_matter(&machine);
    machine.admin_owned = vec![(m1, owner())];
    machine.data_dir_owners = found.map(|sid| Sid::parse(sid).unwrap());
    let removed = remove(&mut machine, &mut acl, true).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    report.ledger
}

#[test]
fn as_an_administrator_a_data_folder_administrators_owns_is_read() {
    // The built-in Administrator, or UAC off: the app made both folders Administrators'.
    let admins = "S-1-5-32-544";
    let work = admins_ledger_work("remove-ledger-admins-data", [admins, admins]);
    // The matter's allow comes off; its `.git` deny's folder fails the fake's path check.
    let read = LedgerWork::AsAdmin {
        removed: 1,
        left: 1,
    };
    assert_eq!(work, read);
}

#[test]
fn as_an_administrator_a_sandbox_folder_a_pool_user_owns_is_never_read() {
    let pool_user = pool_sid(1001);
    let work = admins_ledger_work("remove-ledger-pool-data", [OWNER, pool_user.as_str()]);
    assert!(matches!(work, LedgerWork::Skipped(_)), "{work:?}");
}
