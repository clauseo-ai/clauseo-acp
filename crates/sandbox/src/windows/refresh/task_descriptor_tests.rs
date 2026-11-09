//! `task_identity.rs`'s descriptor rule, on every OS: every entry the set-up gives must be on
//! the task exactly, and the only extra entries allowed are allow entries for SYSTEM or
//! Administrators, as Windows adds at registration (the VM's read-back, Server 2025). The
//! descriptor the definition carries (the XML's `<SecurityDescriptor>`) is held to the same rule,
//! and a definition carrying none is changed. And the readiness check's words over every task a
//! per-user install needs: missing over changed over unchecked, whichever task comes first.

use super::task_identity::*;
use super::task_identity_tests::{
    ADMINISTRATORS, LAWYER, Reader, SYSTEM, compare, registered, with_entries,
};
use super::{SystemTask, USER_TASKS};
use crate::SetupState;
use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};

/// `FILE_GENERIC_READ` as Windows reads it back for the `(A;;FR;;;SY)` it adds.
const FILE_READ: u32 = 0x0012_0089;
/// A pool user's SID.
const POOL: &str = "S-1-5-21-1-2-3-1100";

/// The sweep task as registered, with `entries` as its DACL and `owner` as its owner.
fn sweep_with(owner: &str, entries: &[(u8, u8, u32, &str)]) -> TaskCheck {
    let expected = registered(SystemTask::Sweep);
    let mut found = expected.clone();
    found.descriptor = with_entries(owner, entries);
    compare(&found, &expected)
}

#[test]
fn the_server_2025_read_back_with_windows_own_system_entry_is_the_same() {
    // Read back by the standard lawyer on the VM (Server 2025) after a registration without
    // `TASK_DONT_ADD_PRINCIPAL_ACE`: SY FA, BA FA, <lawyer> 0x1200a9, SY FR.
    let vm = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, 0x0012_00A9, LAWYER),
        (0, 0, FILE_READ, SYSTEM),
    ];
    assert_eq!(sweep_with(ADMINISTRATORS, &vm), TaskCheck::Same);
}

#[test]
fn an_extra_entry_for_administrators_is_the_same() {
    let entries = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, LAWYER),
        (0, 0, FILE_READ, ADMINISTRATORS),
    ];
    assert_eq!(sweep_with(ADMINISTRATORS, &entries), TaskCheck::Same);
}

#[test]
fn an_extra_entry_for_users_everyone_or_a_pool_user_is_changed() {
    let changed = TaskCheck::Differs(Differs::Descriptor);
    for (who, rights) in [
        ("S-1-5-32-545", FULL_ACCESS),
        ("S-1-5-32-545", FILE_READ),
        ("S-1-1-0", FILE_READ),
        (POOL, READ_EXECUTE),
    ] {
        let entries = [
            (0, 0, FULL_ACCESS, SYSTEM),
            (0, 0, FULL_ACCESS, ADMINISTRATORS),
            (0, 0, READ_EXECUTE, LAWYER),
            (0, 0, rights, who),
        ];
        assert_eq!(sweep_with(ADMINISTRATORS, &entries), changed, "{who}");
    }
}

#[test]
fn a_wider_lawyer_entry_is_changed() {
    let changed = TaskCheck::Differs(Differs::Descriptor);
    let base = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
    ];
    // In place of the set-up's entry, or beside it: write (`FILE_GENERIC_WRITE`) or everything.
    for wider in [READ_EXECUTE | 0x0012_0116, FULL_ACCESS] {
        let replaced = [base[0], base[1], (0, 0, wider, LAWYER)];
        assert_eq!(sweep_with(ADMINISTRATORS, &replaced), changed, "{wider:#x}");
        let beside = [
            base[0],
            base[1],
            (0, 0, READ_EXECUTE, LAWYER),
            (0, 0, wider, LAWYER),
        ];
        assert_eq!(sweep_with(ADMINISTRATORS, &beside), changed, "{wider:#x}");
    }
}

#[test]
fn a_missing_or_different_entry_of_the_set_ups_or_another_owner_is_changed() {
    let changed = TaskCheck::Differs(Differs::Descriptor);
    let system = (0, 0, FULL_ACCESS, SYSTEM);
    let admins = (0, 0, FULL_ACCESS, ADMINISTRATORS);
    let lawyer = (0, 0, READ_EXECUTE, LAWYER);
    let cases: [&[(u8, u8, u32, &str)]; 6] = [
        // SYSTEM's full control missing, with Windows' read entry left.
        &[admins, lawyer, (0, 0, FILE_READ, SYSTEM)],
        &[system, lawyer],
        &[system, admins],
        // The lawyer's entry narrowed to read only: it can't start the task.
        &[system, admins, (0, 0, FILE_READ, LAWYER)],
        // Administrators narrowed.
        &[system, (0, 0, READ_EXECUTE, ADMINISTRATORS), lawyer],
        // A deny entry, even for SYSTEM.
        &[system, admins, lawyer, (1, 0, FILE_READ, SYSTEM)],
    ];
    for entries in cases {
        assert_eq!(sweep_with(ADMINISTRATORS, entries), changed, "{entries:?}");
    }
    assert_eq!(sweep_with(LAWYER, &[system, admins, lawyer]), changed);
    assert_eq!(sweep_with(SYSTEM, &[system, admins, lawyer]), changed);
}

#[test]
fn a_definition_without_its_descriptor_or_with_another_is_changed() {
    let expected = registered(SystemTask::Stop);
    let changed = TaskCheck::Differs(Differs::Descriptor);
    // As the VM read `SandboxStop` back after `\Clauseo` was made again: no
    // `<SecurityDescriptor>` in the definition, the task's own DACL the same.
    let mut found = expected.clone();
    found.definition_descriptor = None;
    assert_eq!(compare(&found, &expected), changed);
    // One that lets Users in.
    let mut found = expected.clone();
    let wider = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, LAWYER),
        (0, 0, FULL_ACCESS, "S-1-5-32-545"),
    ];
    found.definition_descriptor = Some(with_entries(ADMINISTRATORS, &wider));
    assert_eq!(compare(&found, &expected), changed);
    // The expected task carries the descriptor it is registered with.
    assert_eq!(
        expected.definition_descriptor.as_deref(),
        Some(expected.descriptor.as_slice())
    );
}

#[test]
fn readiness_says_missing_over_changed_over_unchecked_across_every_task() {
    assert_eq!(
        USER_TASKS,
        [SystemTask::Refresh, SystemTask::Stop, SystemTask::Sweep]
    );
    let at = |task: SystemTask| USER_TASKS.iter().position(|t| *t == task).unwrap();
    // Stop changed, sweep missing: missing, though the stop task is checked first.
    let mut reader = Reader::adopted();
    if let Ok(stop) = &mut reader.tasks[at(SystemTask::Stop)].1 {
        stop.settings.enabled = false;
    }
    let mut reader = reader.without(SystemTask::Sweep);
    assert_eq!(tasks_repair(&mut reader), Some(TASK_MISSING));
    // Refresh unreadable, sweep changed: changed.
    let mut reader = Reader::adopted();
    reader.tasks[at(SystemTask::Refresh)].1 = Err(ReadError::Failed("down".to_owned()));
    reader.tasks[at(SystemTask::Sweep)].1 = Err(ReadError::Refused);
    assert_eq!(tasks_repair(&mut reader), Some(TASK_CHANGED));
    // Only the refresh task missing is missing too.
    let mut reader = Reader::adopted().without(SystemTask::Refresh);
    assert_eq!(tasks_repair(&mut reader), Some(TASK_MISSING));
    assert_eq!(
        ready_with_tasks(SetupState::Ready, &mut reader),
        SetupState::Failed(TASK_MISSING.to_owned())
    );
    assert_eq!(tasks_repair(&mut Reader::adopted()), None);
}

#[test]
fn a_deleted_task_or_folder_reads_as_missing_for_either_hresult() {
    // The VM read a deleted `\Clauseo` as `ERROR_FILE_NOT_FOUND`; `ERROR_PATH_NOT_FOUND` is
    // the other. Each makes `read_tasks` give `None` for the task, which readiness calls
    // missing; `E_ACCESSDENIED` is a refused read instead.
    for hresult in [0x8007_0002_u32, 0x8007_0003] {
        assert!(not_found(hresult), "{hresult:#x}");
        let mut reader = Reader::adopted().without(SystemTask::Sweep);
        assert_eq!(
            tasks_repair(&mut reader),
            Some(TASK_MISSING),
            "{hresult:#x}"
        );
    }
    assert!(!not_found(0x8007_0005));
    assert_eq!(
        ReadError::of(0x8007_0005, String::new()),
        ReadError::Refused
    );
}
