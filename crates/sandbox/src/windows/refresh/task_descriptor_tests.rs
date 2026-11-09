//! `task_identity.rs`'s descriptor rule, on every OS: every entry the set-up gives must be on
//! the task exactly, and the only extra entries allowed are allow entries for SYSTEM or
//! Administrators, as Windows adds at registration (the VM's read-back, Server 2025). The
//! descriptor the definition carries (the XML's `<SecurityDescriptor>`) is held to the same rule;
//! a definition carrying none rests on the task's own. The DACL must be protected, and the
//! refresh task's logon trigger names the owner by SID, however Windows writes the user back.
//! And the readiness check's words over every task a per-user install needs: missing over
//! changed over unchecked, whichever task comes first.

use super::task_identity::*;
use super::task_identity_tests::{
    ADMINISTRATORS, LAWYER, Reader, SYSTEM, compare, registered, with_entries,
};
use super::{SystemTask, USER_TASKS};
use crate::SetupState;
use crate::windows::admin_copy::fake::descriptor;
use crate::windows::admin_copy::{FULL_ACCESS, READ_EXECUTE};
use crate::windows::sids::Sid;

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
fn a_definition_without_a_descriptor_rests_on_the_tasks_own() {
    let expected = registered(SystemTask::Stop);
    let changed = TaskCheck::Differs(Differs::Descriptor);
    // An earlier --adopt's task, from before registration set the definition's descriptor:
    // none carried, the task's own descriptor the set-up's. The same.
    let mut found = expected.clone();
    found.definition_descriptor = None;
    assert_eq!(compare(&found, &expected), TaskCheck::Same);
    // None carried and the task's own descriptor wrong: changed.
    let users = (0, 0, FULL_ACCESS, "S-1-5-32-545");
    let set_ups = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, LAWYER),
    ];
    let wider = [set_ups[0], set_ups[1], set_ups[2], users];
    found.descriptor = with_entries(ADMINISTRATORS, &wider);
    assert_eq!(compare(&found, &expected), changed);
    // One carried that lets Users in: changed, whatever the task's own says.
    let mut found = expected.clone();
    found.definition_descriptor = Some(with_entries(ADMINISTRATORS, &wider));
    assert_eq!(compare(&found, &expected), changed);
    // The expected task carries the descriptor it is registered with.
    assert_eq!(
        expected.definition_descriptor.as_deref(),
        Some(expected.descriptor.as_slice())
    );
}

#[test]
fn a_dacl_that_isnt_protected_is_changed() {
    let expected = registered(SystemTask::Sweep);
    let set_ups = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, LAWYER),
    ];
    let mut found = expected.clone();
    found.descriptor = descriptor(ADMINISTRATORS, Some(&set_ups), false);
    assert_eq!(
        compare(&found, &expected),
        TaskCheck::Differs(Differs::Descriptor)
    );
    found.descriptor = descriptor(ADMINISTRATORS, Some(&set_ups), true);
    assert_eq!(compare(&found, &expected), TaskCheck::Same);
}

#[test]
fn the_refresh_logon_trigger_compares_by_sid_however_windows_writes_the_user() {
    let expected = registered(SystemTask::Refresh);
    let lawyer_name = r"CLAUSEO-VM\lawyer";
    let lookup = |name: &str| (name == lawyer_name).then(|| Sid::parse(LAWYER).unwrap());
    // The SID as written, and the account's name resolving to the same SID: the same.
    for written in [LAWYER, lawyer_name] {
        let mut found = expected.clone();
        found.triggers = vec![TaskTrigger::Logon(principal_sid(written, lookup))];
        assert_eq!(compare(&found, &expected), TaskCheck::Same, "{written}");
    }
    // Another user, by SID or by a name that resolves to theirs: changed.
    let other = "S-1-5-21-1-2-3-1002";
    let other_name = r"CLAUSEO-VM\other";
    let lookup = |name: &str| (name == other_name).then(|| Sid::parse(other).unwrap());
    for written in [other, other_name, r"CLAUSEO-VM\nobody"] {
        let mut found = expected.clone();
        found.triggers = vec![TaskTrigger::Logon(principal_sid(written, lookup))];
        let changed = TaskCheck::Differs(Differs::Triggers);
        assert_eq!(compare(&found, &expected), changed, "{written}");
    }
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
