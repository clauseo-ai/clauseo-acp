//! `task_identity.rs`'s descriptor rule, on every OS: every entry the set-up gives must be on
//! the task exactly, and the only extra entries allowed are allow entries for SYSTEM or
//! Administrators, as Windows adds at registration (the VM's read-back, Server 2025). The
//! descriptor the definition carries (the XML's `<SecurityDescriptor>`) is held to the same rule;
//! a definition carrying none rests on the task's own. The DACL's control flags aren't compared
//! (an earlier build's task updated in place reads back unprotected), and names (the principal
//! as `SYSTEM`, the refresh task's logon trigger as `MACHINE\user`) are compared with the
//! expected SID's local name, a failed lookup being couldn't-check.
//! And the readiness check's words over every task a per-user install needs: missing over
//! changed over unchecked, whichever task comes first.

use super::task_identity::*;
use super::task_identity_tests::{
    ADMINISTRATORS, LAWYER, Reader, SYSTEM, UNANSWERED, compare, local_names, registered,
    with_entries, with_group,
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
    found.definition_descriptor = Carried::Absent;
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
    found.definition_descriptor = Carried::Descriptor(with_entries(ADMINISTRATORS, &wider));
    assert_eq!(compare(&found, &expected), changed);
    // One carried that isn't a string or doesn't parse: changed, never skipped.
    let mut found = expected.clone();
    found.definition_descriptor = Carried::Unreadable;
    assert_eq!(compare(&found, &expected), changed);
    // The expected task carries the descriptor it is registered with.
    assert_eq!(
        expected.definition_descriptor,
        Carried::Descriptor(expected.descriptor.clone())
    );
}

#[test]
fn an_earlier_builds_task_updated_in_place_is_the_same() {
    // The VM (Server 2025): Stop and Sweep from an earlier build, updated in place, read back
    // D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;<lawyer>)(A;;FR;;;SY), no P, no AI. The control
    // flags aren't compared; the entry rule holds.
    let expected = registered(SystemTask::Sweep);
    let upgraded = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, LAWYER),
        (0, 0, 0x0012_0089, SYSTEM),
    ];
    let mut found = expected.clone();
    found.descriptor = descriptor(ADMINISTRATORS, Some(&upgraded), false);
    found.definition_descriptor = Carried::Absent;
    assert_eq!(compare(&found, &expected), TaskCheck::Same);
    found.descriptor = descriptor(ADMINISTRATORS, Some(&upgraded), true);
    assert_eq!(compare(&found, &expected), TaskCheck::Same);
    // Unprotected or not, an entry for Users is still a change.
    let users = (0, 0, FULL_ACCESS, "S-1-5-32-545");
    let wider = [upgraded[0], upgraded[1], upgraded[2], users];
    found.descriptor = descriptor(ADMINISTRATORS, Some(&wider), false);
    assert_eq!(
        compare(&found, &expected),
        TaskCheck::Differs(Differs::Descriptor)
    );
}

#[test]
fn the_shipped_releases_stop_and_refresh_tasks_are_the_same() {
    // The VM's untouched per-user install of the shipped release: both tasks'
    // live descriptor O:BAG:<domain>-513D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;<lawyer>)
    // (A;;FR;;;SY): no P, no AI, Windows' SY FR, and a group the set-up names none of, so
    // isn't compared. The principal S-1-5-18; Stop's XML carries the set-up's descriptor,
    // Refresh's none, and Refresh's logon trigger names the lawyer as MACHINE\user.
    let shipped = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, 0x0012_00a9, LAWYER),
        (0, 0, 0x0012_0089, SYSTEM),
    ];
    let live = with_group(
        descriptor(ADMINISTRATORS, Some(&shipped), false),
        "S-1-5-21-1-2-3-513",
    );
    let stop_expected = registered(SystemTask::Stop);
    let mut stop = stop_expected.clone();
    stop.principal = account_text(SYSTEM);
    stop.descriptor = live.clone();
    assert_eq!(compare(&stop, &stop_expected), TaskCheck::Same);
    let refresh_expected = registered(SystemTask::Refresh);
    let mut refresh = refresh_expected.clone();
    refresh.descriptor = live;
    refresh.definition_descriptor = Carried::Absent;
    refresh.triggers = vec![TaskTrigger::Logon(account_text(r"CLAUSEO-VM\lawyer"))];
    assert_eq!(compare(&refresh, &refresh_expected), TaskCheck::Same);
}

#[test]
fn names_compare_with_the_expected_sids_local_name_and_never_ask_further() {
    let expected = registered(SystemTask::Refresh);
    // The principal through COM: `SYSTEM` against S-1-5-18, any case or prefix.
    for written in [
        "SYSTEM",
        "system",
        r"NT AUTHORITY\SYSTEM",
        "LocalSystem",
        SYSTEM,
    ] {
        let mut found = expected.clone();
        found.principal = account_text(written);
        assert_eq!(compare(&found, &expected), TaskCheck::Same, "{written}");
    }
    // The refresh task's logon trigger, registered by SID, read back as `MACHINE\user`.
    for written in [
        r"CLAUSEO-VM\lawyer",
        r"clauseo-vm\LAWYER",
        r".\lawyer",
        "lawyer",
        LAWYER,
    ] {
        let mut found = expected.clone();
        found.triggers = vec![TaskTrigger::Logon(account_text(written))];
        assert_eq!(compare(&found, &expected), TaskCheck::Same, "{written}");
    }
    // Another user, by SID or by name, or the lawyer's name in another domain: changed.
    let changed = TaskCheck::Differs(Differs::Triggers);
    for written in [
        "S-1-5-21-1-2-3-1002",
        r"CLAUSEO-VM\other",
        r"CORP\lawyer",
        "",
    ] {
        let mut found = expected.clone();
        found.triggers = vec![TaskTrigger::Logon(account_text(written))];
        assert_eq!(compare(&found, &expected), changed, "{written}");
    }
    let mut found = expected.clone();
    found.principal = account_text(r"CLAUSEO-VM\lawyer");
    assert_eq!(
        compare(&found, &expected),
        TaskCheck::Differs(Differs::Principal)
    );
    // A local lookup that fails can't tell: couldn't-check, never changed. A SID needs none.
    let mut down = |_: &Sid| Err("LookupAccountSidW: RPC server unavailable".to_owned());
    let mut found = expected.clone();
    found.triggers = vec![TaskTrigger::Logon(account_text(r"CLAUSEO-VM\lawyer"))];
    assert!(check(Ok(Some(&found)), &expected, &mut down).is_err());
    let mut nameless = |_: &Sid| Ok(None);
    assert!(check(Ok(Some(&found)), &expected, &mut nameless).is_err());
    let unanswered = same_account(r"CLAUSEO-VM\lawyer", UNANSWERED, &mut local_names);
    assert!(unanswered.is_err());
    let checked = check(Ok(Some(&expected)), &expected, &mut down);
    assert_eq!(checked, Ok(TaskCheck::Same));
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
    // Refresh unreadable, sweep changed: changed (the refresh task isn't the launch's).
    let mut reader = Reader::adopted();
    reader.tasks[at(SystemTask::Refresh)].1 = Err(ReadError::Failed("down".to_owned()));
    reader.tasks[at(SystemTask::Sweep)].1 = Err(ReadError::Refused);
    assert_eq!(tasks_repair(&mut reader), Some(TASK_CHANGED));
    assert_eq!(tasks_repair(&mut Reader::adopted()), None);
}

#[test]
fn the_refresh_task_alone_never_gates_a_launch_and_asks_for_a_repair() {
    let at = |task: SystemTask| USER_TASKS.iter().position(|t| *t == task).unwrap();
    // The refresh task alone, missing, changed or unreadable: nothing for the launch's check
    // (`tasks_repair`, which `logon_check` reads), and a repair needed for readiness.
    let repair = SetupState::RepairNeeded(REFRESH_TASK_MISSING.to_owned());
    let at_refresh = at(SystemTask::Refresh);
    let refresh_states: [fn(&mut Reader); 3] = [
        |r| r.tasks.retain(|(t, _)| *t != SystemTask::Refresh),
        |r| {
            if let Ok(t) = &mut r.tasks[0].1 {
                t.settings.enabled = false;
            }
        },
        |r| r.tasks[0].1 = Err(ReadError::Failed("down".to_owned())),
    ];
    assert_eq!(at_refresh, 0);
    for change in refresh_states {
        let mut reader = Reader::adopted();
        change(&mut reader);
        assert_eq!(tasks_repair(&mut reader), None);
        assert_eq!(ready_with_tasks(SetupState::Ready, &mut reader), repair);
    }
    assert_eq!(
        REFRESH_TASK_MISSING,
        "Clauseo's update task is missing, so its safety program won't get updates. The AI \
         tools still work. Repair Clauseo to put the task back."
    );
    // With the sweep task missing too: the clean-up words, for the launch and for readiness.
    let mut reader = Reader::adopted()
        .without(SystemTask::Refresh)
        .without(SystemTask::Sweep);
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
