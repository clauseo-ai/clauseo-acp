//! `--remove` as an administrator for an absent owner takes back each read of another matter
//! (R18) where its item is now, found by its recorded file ID: a moved item loses the allow where
//! it went, a deleted one counts as removed, a new item at the recorded path is left alone, and a
//! read from before file IDs is removed by its path. Each only inside a matter folder the ledger
//! records, behind the folder's path check; never a walk, an add or a ledger write.

use std::path::Path;

use clauseo_protocol::MatterId;

use super::fake::OTHER_OWNER;
use super::fake_acl::Key;
use super::remove::pool;
use super::remove_ledger::{grant, matter_one, matter_two, matters, named, pool_sid};
use super::*;
use crate::windows::acl_plan::{AdmittedPlan, Grant, Inherit};
use crate::windows::admit::PathKind;
use crate::windows::ledger::{Ledger, RecordedFile, ledger_path};
use crate::windows::sids::Reserved;
use crate::windows::writer::FileId;

/// Records `grants` for `matter` in the ledger in `data_dir`, pending; each one at an index in
/// `files` records the file at its path, with an ID of its own.
fn record(data_dir: &Path, matter: MatterId, grants: &[Grant], files: &[usize]) {
    let mut ledger = Ledger::open(ledger_path(data_dir), &named(), &Reserved::new([])).unwrap();
    let mut plan = AdmittedPlan::from_grants(matter, grants.to_vec());
    ledger.record_pending(&mut plan).unwrap();
    for (n, at) in (1u8..).zip(files) {
        let id = plan.entries()[*at];
        let file = RecordedFile::new(
            grants[*at].path.clone(),
            FileId {
                volume: 7,
                id: [n; 16],
            },
        );
        ledger.set_file(&plan, id, &file).unwrap();
    }
}

#[test]
fn as_an_administrator_each_read_is_taken_back_by_its_file_id_wherever_its_item_went() {
    let admin = Sid::parse(OTHER_OWNER).unwrap();
    for invoker in [Some(admin), None] {
        let (mut machine, mut acl) = pool("remove-ledger-reads", invoker);
        let data_dir = machine.m.data_dir();
        let m2 = matters(&machine).join("m2");
        let elsewhere = machine.m.dir.path().join("elsewhere");
        let user = pool_sid(1001);
        let read = |path: &Path, inherit| {
            grant(
                path,
                &user,
                Rights::ReadAndExecute,
                inherit,
                PathKind::OtherMatterRead,
            )
        };
        let reads = [
            // Moved during the run: both of the folder's allows go where it went.
            read(&m2.join("docs"), Inherit::ThisOnly),
            read(&m2.join("docs"), Inherit::FilesInFolder),
            // Deleted: nothing left to take back.
            read(&m2.join("gone.txt"), Inherit::ThisOnly),
            // Replaced: the old file, moved aside, loses it; the new one keeps its own entries.
            read(&m2.join("brief.txt"), Inherit::ThisOnly),
            // From before file IDs: removed by its path.
            read(&m2.join("old.txt"), Inherit::ThisOnly),
            // Moved out of every recorded matter folder: left.
            read(&m2.join("away.txt"), Inherit::ThisOnly),
            // In a folder that fails the path check (a link, say): left.
            read(&m2.join("linked/x.txt"), Inherit::ThisOnly),
        ];
        record(&data_dir, matter_one(), &reads, &[0, 1, 2, 3, 5, 6]);
        // The other matter's fence: it makes `m2` a matter folder the ledger records.
        let fence = grant(
            &m2,
            &user,
            Rights::DenyReadWrite,
            Inherit::Tree,
            PathKind::OtherMatter,
        );
        record(&data_dir, matter_two(), &[fence], &[]);
        acl.acl
            .moved
            .insert(m2.join("docs"), Some(m2.join("moved")));
        acl.acl.moved.insert(m2.join("gone.txt"), None);
        acl.acl
            .moved
            .insert(m2.join("brief.txt"), Some(m2.join("brief-old.txt")));
        acl.acl
            .moved
            .insert(m2.join("away.txt"), Some(elsewhere.join("away.txt")));
        let ace = |path: &Path, inherit| -> Key {
            let sid = user.as_str().to_owned();
            (path.to_owned(), sid, Rights::ReadAndExecute, inherit)
        };
        let gone = [
            ace(&m2.join("moved"), Inherit::ThisOnly),
            ace(&m2.join("moved"), Inherit::FilesInFolder),
            ace(&m2.join("brief-old.txt"), Inherit::ThisOnly),
            ace(&m2.join("old.txt"), Inherit::ThisOnly),
        ];
        let kept = [
            // The new item at the recorded path, with an entry of the lawyer's making.
            ace(&m2.join("brief.txt"), Inherit::ThisOnly),
            ace(&elsewhere.join("away.txt"), Inherit::ThisOnly),
            ace(&m2.join("linked/x.txt"), Inherit::ThisOnly),
        ];
        acl.acl.aces.extend(gone.iter().chain(&kept).cloned());
        machine.admin_owned = vec![(m2.clone(), owner())];
        let bytes = std::fs::read(ledger_path(&data_dir)).unwrap();

        let removed = remove(&mut machine, &mut acl, true).unwrap();
        let Removed::All(report) = removed else {
            panic!("{removed:?}");
        };
        // Eight entries: the moved folder's two, the deleted, the replaced and the older read go.
        assert_eq!(
            report.ledger,
            LedgerWork::AsAdmin {
                removed: 5,
                left: 3
            }
        );
        for key in &gone {
            assert!(!acl.acl.aces.contains(key), "{key:?}");
        }
        for key in &kept {
            assert!(acl.acl.aces.contains(key), "{key:?}");
        }
        let lines = machine.lines();
        let checked: Vec<&str> = lines
            .iter()
            .filter_map(|l| l.strip_prefix("admin_target "))
            .collect();
        assert_eq!(checked, ["matters/m2", "matters/m2/linked"]);
        let ledger_lines: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("matters/") || l.contains("elsewhere"))
            .filter(|l| !l.starts_with("admin_target "))
            .collect();
        assert_eq!(
            ledger_lines,
            [
                "remove_recorded matters/m2/docs 1001",
                "remove_recorded matters/m2/docs 1001",
                "remove_recorded matters/m2/brief.txt 1001",
                "remove matters/m2/old.txt 1001",
            ],
            "only removals, each by the recorded file but the older read; no walk, no add"
        );
        for kind in ["add ", "restore_label ", "reclaim "] {
            assert!(!lines.iter().any(|l| l.starts_with(kind)), "{lines:?}");
        }
        assert!(!machine.acted);
        assert_eq!(std::fs::read(ledger_path(&data_dir)).unwrap(), bytes);
        assert_eq!(machine.m.state.record, None);
    }
}

#[test]
fn as_an_administrator_an_older_ledgers_reads_with_no_file_are_removed_by_their_paths() {
    let (mut machine, mut acl) = pool("remove-ledger-older-reads", None);
    let data_dir = machine.m.data_dir();
    let m2 = matters(&machine).join("m2");
    let user = pool_sid(1001);
    let read = |rel: &str, inherit| {
        let kind = PathKind::OtherMatterRead;
        grant(&m2.join(rel), &user, Rights::ReadAndExecute, inherit, kind)
    };
    let reads = [
        read("docs", Inherit::ThisOnly),
        read("docs", Inherit::FilesInFolder),
        read("brief.txt", Inherit::ThisOnly),
    ];
    // A ledger from before file IDs: no read records its file.
    record(&data_dir, matter_one(), &reads, &[]);
    let fence = grant(
        &m2,
        &user,
        Rights::DenyReadWrite,
        Inherit::Tree,
        PathKind::OtherMatter,
    );
    record(&data_dir, matter_two(), &[fence], &[]);
    for g in &reads {
        acl.acl.add(g).unwrap();
    }
    machine.admin_owned = vec![(m2.clone(), owner())];

    let removed = remove(&mut machine, &mut acl, true).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(
        report.ledger,
        LedgerWork::AsAdmin {
            removed: 3,
            left: 1
        }
    );
    for g in &reads {
        assert!(
            !acl.acl.has(g.path(), &user, Rights::ReadAndExecute),
            "{g:?}"
        );
    }
    let lines = machine.lines();
    assert!(
        !lines.iter().any(|l| l.starts_with("remove_recorded ")),
        "{lines:?}"
    );
    assert_eq!(machine.m.state.record, None);
}
