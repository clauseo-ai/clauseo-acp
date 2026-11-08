//! The SYSTEM tasks' definitions (`rules.rs`), on every OS: the stop and sweep tasks are the
//! refresh task with their own names, arguments and limits, the same principal and descriptor,
//! and no logon start; the uninstaller's removal takes them all down before the copy they run,
//! and a per-machine install's set-up registers and removes the stop and sweep tasks.

use super::rules::{MSI_EVENT_IDS, Trigger, msi_event_query};
use super::*;
use crate::windows::setup::args::{Mode, parse};

#[test]
fn the_stop_and_sweep_tasks_are_the_refresh_task_but_for_name_argument_limit_and_trigger() {
    assert_eq!(
        TASKS,
        [SystemTask::Refresh, SystemTask::Stop, SystemTask::Sweep]
    );
    let refresh = SystemTask::Refresh;
    let stop = SystemTask::Stop;
    let sweep = SystemTask::Sweep;
    assert_eq!(
        (
            refresh.name(),
            refresh.argument(),
            refresh.execution_limit()
        ),
        ("SandboxRefresh", "--refresh", "PT5M")
    );
    assert_eq!(
        (stop.name(), stop.argument(), stop.execution_limit()),
        ("SandboxStop", "--stop", "PT2M")
    );
    assert_eq!((STOP_ARG, STOP_EXECUTION_LIMIT), ("--stop", "PT2M"));
    assert_eq!(
        (sweep.name(), sweep.argument(), sweep.execution_limit()),
        ("SandboxSweep", "--sweep-tasks", "PT2M")
    );
    assert_eq!(
        (SWEEP_ARG, SWEEP_EXECUTION_LIMIT),
        ("--sweep-tasks", "PT2M")
    );
    // One fixed argument each, with no value and nothing to quote: the action passes nothing
    // a caller chose.
    for task in TASKS {
        let argument = task.argument();
        assert!(argument.starts_with("--"), "{argument}");
        assert!(!argument.contains([' ', '=', '"', '\t']), "{argument}");
        assert_ne!(task.description(), "");
    }
    // The refresh also starts at the owner's logon; the stop and the sweep only when asked. A
    // sweep asked for while one runs is queued after it; the others' second runs are dropped.
    assert!(refresh.at_logon());
    assert!(!stop.at_logon());
    assert!(!sweep.at_logon());
    assert_eq!(
        TASKS.map(SystemTask::queues),
        [false, false, true],
        "only the sweep queues"
    );
    // The argument each task passes is the one the exe parses as its mode, and alone.
    let mode = |task: SystemTask| parse([std::ffi::OsString::from(task.argument())]);
    assert_eq!(mode(refresh), Ok(Mode::Refresh));
    assert_eq!(mode(stop), Ok(Mode::Stop));
    assert_eq!(mode(sweep), Ok(Mode::SweepTasks));
}

#[test]
fn the_triggers_pass_nothing_but_the_fixed_argument() {
    // The refresh starts at the owner's logon and on Windows Installer's events (WSL's deny
    // comes back after a WSL update); the stop and the sweep only on demand.
    assert_eq!(
        SystemTask::Refresh.triggers(),
        [Trigger::Logon, Trigger::MsiEvents]
    );
    assert_eq!(SystemTask::Stop.triggers(), []);
    assert_eq!(SystemTask::Sweep.triggers(), []);
    assert_eq!(
        MSI_EVENT_IDS,
        [1022, 1033, 1035, 1040, 1042, 11707, 11724, 11728]
    );
    // The subscription is fixed text: the Application log, the MsiInstaller provider, those
    // IDs, and nothing else. `register_task` sets it alone: no value queries, so no event data
    // reaches the action.
    let query = msi_event_query();
    assert_eq!(
        query,
        "<QueryList><Query Id=\"0\" Path=\"Application\"><Select Path=\"Application\">\
         *[System[Provider[@Name='MsiInstaller'] and (EventID=1022 or EventID=1033 or \
         EventID=1035 or EventID=1040 or EventID=1042 or EventID=11707 or EventID=11724 or \
         EventID=11728)]]</Select></Query></QueryList>"
    );
    assert!(!query.contains("$("), "{query}");
    // Whatever starts it, the action's argument is the one fixed argument, with no event value
    // to substitute (the Task Scheduler's `$(name)`).
    for task in TASKS {
        assert!(!task.argument().contains('$'), "{task:?}");
    }
    assert_eq!(SystemTask::Refresh.argument(), REFRESH_ARG);
}

#[test]
fn both_tasks_take_the_one_descriptor_the_owner_may_read_and_execute() {
    // `register_task` passes `task_sddl(owner)` to the folder and to every task alike: no task
    // takes a descriptor of its own. The owner reads and executes (FRFX): starting a task needs
    // execute (CI run 42416074600), which also ends one. It can't write, delete or change the
    // DACL. No one else but SYSTEM and Administrators is named.
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let sddl = task_sddl(&owner);
    assert_eq!(
        sddl,
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)"
    );
    let aces: Vec<&str> = sddl.split('(').skip(1).collect();
    assert_eq!(aces.len(), 3);
    assert!(aces[2].starts_with("A;;FRFX;;;"), "{}", aces[2]);
    for wider in ["FA", "FW", "GA", "GW", "WD", "WO", "SD"] {
        assert!(!aces[2].contains(wider), "{wider}: {}", aces[2]);
    }
}

#[test]
fn the_removal_takes_every_task_down_after_the_values_and_before_the_copy() {
    assert_eq!(
        TEARDOWN,
        [
            Teardown::AdminValues,
            Teardown::Task(SystemTask::Refresh),
            Teardown::Task(SystemTask::Stop),
            Teardown::Task(SystemTask::Sweep),
            Teardown::Staging,
            Teardown::AdminFolder,
        ]
    );
    for task in TASKS {
        let at = TEARDOWN
            .iter()
            .position(|step| *step == Teardown::Task(task));
        let at = at.unwrap_or_else(|| panic!("{task:?} isn't taken down"));
        let folder = TEARDOWN
            .iter()
            .position(|step| *step == Teardown::AdminFolder);
        assert!(Some(at) < folder, "{task:?} goes before the copy");
        assert!(at > 0, "{task:?} goes after the values");
    }
}

#[test]
fn a_per_machine_set_up_registers_and_removes_the_stop_and_sweep_tasks() {
    // `register_stop_task` and `unregister_stop_task` (`windows_machine.rs`,
    // `remove/windows_removal.rs`) go over these; the refresh is `--adopt`'s alone.
    assert_eq!(POOL_TASKS, [SystemTask::Stop, SystemTask::Sweep]);
    for task in POOL_TASKS {
        assert!(TASKS.contains(&task), "{task:?}");
        assert!(TEARDOWN.contains(&Teardown::Task(task)), "{task:?}");
    }
}

#[test]
fn the_action_names_the_admin_copy_in_quotes() {
    let exe = Path::new(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe");
    assert_eq!(
        action_path(exe),
        r#""C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe""#
    );
}
