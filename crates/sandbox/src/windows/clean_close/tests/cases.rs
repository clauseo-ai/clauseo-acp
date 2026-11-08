//! Each condition that makes a launch walk in full, and when the record is written and removed.

use super::super::fake::found;
use super::*;

fn closed(test: &str) -> Scene {
    let s = Scene::new(test);
    assert_eq!(s.close(), Closed::Written);
    s
}

#[test]
fn no_record_walks_in_full() {
    let s = Scene::new("clean-no-record");
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NoRecord));
}

#[test]
fn a_journal_id_change_walks_in_full() {
    let mut s = closed("clean-journal-id");
    s.journal.journal += 1;
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::JournalChanged)
    );
}

#[test]
fn a_wrapped_journal_walks_in_full() {
    let mut s = closed("clean-wrap");
    s.journal.first = s.journal.end() + 5;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Wrapped));
}

#[test]
fn records_lost_while_reading_walk_in_full() {
    let mut s = closed("clean-lost");
    s.journal.read_fails = Some(JournalError::Lost);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Wrapped));
}

#[test]
fn a_reset_or_rolled_back_journal_walks_in_full() {
    let mut s = closed("clean-rollback");
    // The volume went back to before the cursor: its next USN is lower.
    s.journal.next = 500;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::RolledBack));
}

#[test]
fn another_root_id_walks_in_full() {
    let mut s = closed("clean-root-id");
    let root = s.matter_root();
    s.journal.roots.get_mut(&root).unwrap().id = Some(4242);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::RootChanged));
}

#[test]
fn another_volume_serial_walks_in_full() {
    let mut s = closed("clean-volume");
    let root = s.root_of(300);
    s.journal.roots.get_mut(&root).unwrap().volume = 8;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::RootChanged));
}

#[test]
fn a_root_that_cant_be_read_walks_in_full() {
    let mut s = closed("clean-root-unread");
    let root = s.root_of(200);
    s.journal.roots.remove(&root);
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::RootUnreadable)
    );
}

#[test]
fn a_changed_plan_walks_in_full() {
    let mut s = closed("clean-plan");
    // Another pool user denied on the matter's folder: the strict sets and the grants differ.
    let mut grants = s.plan.grants().to_vec();
    grants.push(Grant {
        path: s.matter_root(),
        sid: Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1099").unwrap(),
        rights: Rights::DenyReadWrite,
        inherit: Inherit::Tree,
        kind: crate::windows::admit::PathKind::Matter,
    });
    s.plan = AdmittedPlan::from_grants(s.matter, grants);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::PlanChanged));
}

#[test]
fn a_volume_that_isnt_ntfs_walks_in_full() {
    let mut s = closed("clean-not-ntfs");
    let root = s.matter_root();
    s.journal.roots.get_mut(&root).unwrap().ntfs = false;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NotNtfs));
    let mut s = closed("clean-refs");
    s.journal.roots.get_mut(&root_of(&s)).unwrap().id = None;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NotNtfs));
}

fn root_of(s: &Scene) -> PathBuf {
    s.matter_root()
}

#[test]
fn a_volume_with_no_journal_walks_in_full() {
    let mut s = closed("clean-no-journal");
    s.journal.query_fails = Some(JournalError::NoJournal);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NoJournal));
}

#[test]
fn a_failed_read_walks_in_full() {
    let mut s = closed("clean-read-fails");
    s.journal.read_fails = Some(JournalError::Failed(io::ErrorKind::Other));
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::ReadFailed));
    let mut s = closed("clean-query-fails");
    s.journal.query_fails = Some(JournalError::Failed(io::ErrorKind::Other));
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::ReadFailed));
}

#[test]
fn an_unsupported_read_walks_in_full() {
    let mut s = closed("clean-unsupported");
    s.journal.read_fails = Some(JournalError::Unsupported);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Unsupported));
    let mut s = closed("clean-unsupported-query");
    s.journal.query_fails = Some(JournalError::Unsupported);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Unsupported));
}

#[test]
fn a_removable_drive_walks_in_full() {
    let mut s = closed("clean-removable");
    let root = s.root_of(300);
    s.journal.roots.get_mut(&root).unwrap().removable = true;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Removable));
}

#[test]
fn an_open_session_walks_in_full() {
    let s = closed("clean-open-session");
    s.open_session();
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::SessionOpen));
}

#[test]
fn a_pending_entry_walks_in_full() {
    let mut s = closed("clean-pending");
    s.leave_pending();
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::LedgerBusy));
}

#[test]
fn a_removing_entry_walks_in_full() {
    let mut s = closed("clean-removing");
    s.leave_removing();
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::LedgerBusy));
}

#[test]
fn a_handback_owed_walks_in_full() {
    let mut s = closed("clean-handback");
    s.owe_handback();
    assert!(
        s.ledger
            .handbacks_owed()
            .iter()
            .any(|o| o.matter == s.matter)
    );
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::HandbackOwed));
}

#[test]
fn more_changed_items_than_the_threshold_walk_in_full() {
    let mut s = closed("clean-threshold");
    // 3 roots of 102 items and 3 roots' own: 309 items, so the threshold is 15.
    for i in 0..15 {
        s.journal.change(100_000 + i, 100, SECURITY, false);
    }
    assert_eq!(s.launch_path(), LaunchPath::Targeted(1));
    let mut s = closed("clean-threshold-over");
    for i in 0..16 {
        s.journal.change(100_000 + i, 100, SECURITY, false);
    }
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::TooMany));
}

#[test]
fn a_folder_whose_place_cant_be_read_walks_in_full() {
    let mut s = closed("clean-unresolved");
    s.journal.places.remove(&101);
    s.journal.change(102, 101, SECURITY, false);
    // Gone is fine (nothing of it is left to walk); a failed lookup is not.
    assert_eq!(s.launch_path(), LaunchPath::Skipped);
    let mut s = closed("clean-unresolved-volume");
    s.journal.change(102, 101, SECURITY, false);
    s.journal
        .places
        .insert(101, found(PathBuf::from("/elsewhere")));
    assert_eq!(s.launch_path(), LaunchPath::Skipped);
}

#[test]
fn the_record_is_removed_at_launch_whatever_the_check_decides() {
    let mut s = closed("clean-removed");
    assert!(s.record_exists());
    s.journal.read_fails = Some(JournalError::Unsupported);
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::Unsupported));
    assert!(!s.record_exists());
    s.journal.read_fails = None;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NoRecord));
    // A skip removes it too: the next launch needs a new clean close.
    assert_eq!(s.close(), Closed::Written);
    assert_eq!(s.launch_path(), LaunchPath::Skipped);
    assert!(!s.record_exists());
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NoRecord));
}

#[test]
fn the_record_is_written_only_at_a_clean_close() {
    let s = Scene::new("clean-only-clean");
    let cursor = s.cursor();
    let ok = || Ok(Walked::default());
    let refused = || {
        Err(crate::Error::AclApply {
            path_kind: crate::windows::admit::PathKind::Matter,
            failure: crate::windows::writer::ApplyFailure::ChildShut,
            at: None,
        })
    };
    let complete = || Ok(Checked::default());
    let partial = || {
        Ok(Checked {
            moved: Vec::new(),
            unscanned: vec![PathBuf::from("sub")],
        })
    };
    let failed = || {
        Err(crate::Error::SetAside {
            failure: crate::windows::SetAsideFailure::Record,
            at: None,
        })
    };
    let not = |why| Closed::NotWritten(why);
    assert_eq!(
        s.close_with(None, &ok(), &complete()),
        not(NotWritten::NoCursor)
    );
    let c = Some(&cursor);
    assert_eq!(
        s.close_with(c, &refused(), &complete()),
        not(NotWritten::RepairFailed)
    );
    assert_eq!(
        s.close_with(c, &ok(), &partial()),
        not(NotWritten::ScanIncomplete)
    );
    assert_eq!(
        s.close_with(c, &ok(), &failed()),
        not(NotWritten::ScanIncomplete)
    );
    assert!(!s.record_exists());
    assert_eq!(s.close_with(c, &ok(), &complete()), Closed::Written);
}

#[test]
fn no_record_while_a_session_is_open_or_the_ledger_owes_a_walk() {
    let s = Scene::new("clean-not-open");
    s.open_session();
    assert_eq!(s.close(), Closed::NotWritten(NotWritten::SessionOpen));
    assert!(!s.record_exists());

    let mut s = Scene::new("clean-not-pending");
    s.leave_pending();
    assert_eq!(s.close(), Closed::NotWritten(NotWritten::LedgerBusy));
    let mut s = Scene::new("clean-not-removing");
    let cursor = s.cursor();
    s.leave_removing();
    let closed = s.close_with(
        Some(&cursor),
        &Ok(Walked::default()),
        &Ok(Checked::default()),
    );
    assert_eq!(closed, Closed::NotWritten(NotWritten::LedgerBusy));
    let mut s = Scene::new("clean-not-handback");
    s.owe_handback();
    assert_eq!(s.close(), Closed::NotWritten(NotWritten::HandbackOwed));
    assert!(!s.record_exists());
}

#[test]
fn no_record_when_a_tree_cant_be_read_or_a_root_changed_after_the_cursor() {
    let mut s = Scene::new("clean-not-tree");
    s.journal.tree_fails = true;
    assert_eq!(s.close(), Closed::NotWritten(NotWritten::TreeUnreadable));
    let mut s = Scene::new("clean-not-root");
    let cursor = s.cursor();
    let root = s.matter_root();
    s.journal.roots.get_mut(&root).unwrap().id = Some(4242);
    let closed = s.close_with(
        Some(&cursor),
        &Ok(Walked::default()),
        &Ok(Checked::default()),
    );
    assert_eq!(closed, Closed::NotWritten(NotWritten::RootsChanged));
    assert!(!s.record_exists());
}

#[test]
fn a_cursor_taken_before_the_repair_replays_what_changed_during_it() {
    let mut s = Scene::new("clean-replay");
    let cursor = s.cursor();
    // The repair (or anyone) changes a file after the cursor, before the record is written.
    s.journal.change(100_007, 100, SECURITY, false);
    let closed = s.close_with(
        Some(&cursor),
        &Ok(Walked::default()),
        &Ok(Checked::default()),
    );
    assert_eq!(closed, Closed::Written);
    assert_eq!(s.launch_path(), LaunchPath::Targeted(1));
}

#[test]
fn another_first_record_at_the_cursor_walks_in_full() {
    // The repair wrote a record past the cursor before the clean-close record was written.
    let mut s = Scene::new("clean-first-time");
    let cursor = s.cursor();
    s.journal.change(100_007, 100, WRITE_CLOSE, false);
    let ok = (&Ok(Walked::default()), &Ok(Checked::default()));
    assert_eq!(s.close_with(Some(&cursor), ok.0, ok.1), Closed::Written);
    let mut restored = s.journal.clone();
    // A restored image wrote another record at the same USN, then went on past the cursor.
    restored.records[0].time += 1;
    s.journal = restored;
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );

    let mut s = Scene::new("clean-first-file");
    let cursor = s.cursor();
    s.journal.change(100_007, 100, WRITE_CLOSE, false);
    assert_eq!(s.close_with(Some(&cursor), ok.0, ok.1), Closed::Written);
    s.journal.records[0].file = 100_008;
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );

    // The record seen at the cursor is gone, though the journal's next USN is past it.
    let mut s = Scene::new("clean-first-gone");
    let cursor = s.cursor();
    s.journal.change(100_007, 100, WRITE_CLOSE, false);
    assert_eq!(s.close_with(Some(&cursor), ok.0, ok.1), Closed::Written);
    s.journal.records.clear();
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );

    // The same record there: no change, a skip.
    let mut s = Scene::new("clean-first-same");
    let cursor = s.cursor();
    s.journal.change(100_007, 100, WRITE_CLOSE, false);
    assert_eq!(s.close_with(Some(&cursor), ok.0, ok.1), Closed::Written);
    assert_eq!(s.launch_path(), LaunchPath::Skipped);
}

#[test]
fn with_no_record_at_the_close_the_first_one_read_must_start_at_the_cursor() {
    let mut s = closed("clean-first-none");
    let cursor = s.journal.end();
    s.journal.records.push(super::super::journal::Change {
        usn: cursor + 8,
        file: 9_999_999,
        parent: 8_888_888,
        reason: WRITE_CLOSE,
        attributes: 0x20,
        time: 5,
    });
    s.journal.next = cursor + 9;
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );
}

#[test]
fn a_close_record_that_cant_be_written_keeps_no_record() {
    let mut s = Scene::new("clean-close-record");
    s.journal.close_fails = true;
    let closed = s.close();
    assert_eq!(
        closed,
        Closed::NotWritten(NotWritten::CloseRecordFailed(
            io::ErrorKind::PermissionDenied
        ))
    );
    assert!(!s.record_exists());
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NoRecord));
}

/// A record at `usn`, written by `file`, past every other: a content write, which doesn't count.
fn record_at(s: &mut Scene, usn: Usn, file: u64) {
    s.journal.records.push(super::super::journal::Change {
        usn,
        file,
        parent: 8_888_888,
        reason: WRITE_CLOSE,
        attributes: 0x20,
        time: 77,
    });
    s.journal.next = usn + 8;
}

/// Round 2 (VM run 5): the cursor 3,968 bytes into a 4 KiB page, the next record at the next
/// page, the rest of the page padding.
#[test]
fn a_first_record_at_the_next_page_boundary_is_the_first_record() {
    let mut s = Scene::new("clean-page");
    s.journal.next = 4096 - 128;
    let cursor = s.cursor();
    record_at(&mut s, 4096, 9_999_999);
    let ok = (&Ok(Walked::default()), &Ok(Checked::default()));
    assert_eq!(s.close_with(Some(&cursor), ok.0, ok.1), Closed::Written);
    assert_eq!(s.launch_path(), LaunchPath::Skipped);

    // None at the close; at the launch the first one is at the next page.
    let mut s = Scene::new("clean-page-none");
    s.journal.next = 4096 - 128;
    assert_eq!(s.close(), Closed::Written);
    record_at(&mut s, 4096, 9_999_999);
    assert_eq!(s.launch_path(), LaunchPath::Skipped);
}

#[test]
fn a_first_record_past_the_next_page_boundary_isnt() {
    // At the close: no record is written.
    let mut s = Scene::new("clean-page-past");
    s.journal.next = 4096 - 128;
    let cursor = s.cursor();
    record_at(&mut s, 4096 + 8, 9_999_999);
    let ok = (&Ok(Walked::default()), &Ok(Checked::default()));
    assert_eq!(
        s.close_with(Some(&cursor), ok.0, ok.1),
        Closed::NotWritten(NotWritten::JournalUnread)
    );
    // At the launch: the full walk.
    let mut s = Scene::new("clean-page-past-launch");
    s.journal.next = 4096 - 128;
    assert_eq!(s.close(), Closed::Written);
    record_at(&mut s, 4096 + 8, 9_999_999);
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );
    // Inside the cursor's page but past the cursor: neither the cursor nor the next page.
    let mut s = Scene::new("clean-page-inside");
    s.journal.next = 4096 - 128;
    assert_eq!(s.close(), Closed::Written);
    record_at(&mut s, 4096 - 64, 9_999_999);
    assert_eq!(
        s.launch_path(),
        LaunchPath::Full(FullReason::FirstRecordDiffers)
    );
}

/// Round 2: a volume that isn't NTFS, read as FAT32 is now (no file ID), is `NotNtfs`.
#[test]
fn a_fat_volume_without_file_ids_is_not_ntfs_not_unreadable() {
    let mut s = closed("clean-fat");
    let root = s.root_of(300);
    let facts = s.journal.roots.get_mut(&root).unwrap();
    facts.ntfs = false;
    facts.id = None;
    facts.volume = 0;
    assert_eq!(s.launch_path(), LaunchPath::Full(FullReason::NotNtfs));
    // And no cursor is taken on one.
    assert_eq!(session_cursor(&s.ledger, s.matter, &s.journal), None);
}
