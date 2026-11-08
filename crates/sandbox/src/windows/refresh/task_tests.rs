//! The SYSTEM tasks' definitions (`rules.rs`), on every OS: the stop task is the refresh task
//! with its own name, argument and limit, the same principal and descriptor, and no logon start;
//! the set-up task queues no runs and is started by the owner or by interactive users but the
//! pool's group; the folder keeps its own descriptor; the uninstaller's removal takes every task
//! down before the copy they run, the set-up task first.

use super::*;
use crate::windows::setup::args::{Mode, parse};

#[test]
fn the_stop_task_is_the_refresh_task_but_for_its_name_argument_and_limit() {
    assert_eq!(TASKS, [SystemTask::Refresh, SystemTask::Stop]);
    let refresh = SystemTask::Refresh;
    let stop = SystemTask::Stop;
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
    // One fixed argument each, with no value and nothing to quote: the action passes nothing
    // a caller chose.
    for task in TASKS {
        let argument = task.argument();
        assert!(argument.starts_with("--"), "{argument}");
        assert!(!argument.contains([' ', '=', '"', '\t']), "{argument}");
        assert_ne!(task.description(), "");
    }
    // The refresh also starts at the owner's logon; the stop only when asked.
    assert!(refresh.at_logon());
    assert!(!stop.at_logon());
    // The argument each task passes is the one the exe parses as its mode, and alone.
    let mode = |task: SystemTask| parse([std::ffi::OsString::from(task.argument())]);
    assert_eq!(mode(refresh), Ok(Mode::Refresh));
    assert_eq!(mode(stop), Ok(Mode::Stop));
    let set_up = SystemTask::SetUp;
    assert_eq!(
        (set_up.name(), set_up.argument(), set_up.execution_limit()),
        ("SandboxSetUp", "--serve-set-up", "PT2H")
    );
    assert_eq!(mode(set_up), Ok(Mode::ServeSetUp));
    assert!(!set_up.at_logon(), "started only on demand: no trigger");
    // No run queues: a start during a set-up run is dropped, and the app starts it again.
    assert!(!set_up.queues() && !refresh.queues() && !stop.queues());
    assert_ne!(set_up.description(), "");
}

#[test]
fn the_set_up_task_is_the_owners_per_user_and_interactive_users_but_the_pool_per_machine() {
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let group = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1050").unwrap();
    assert_eq!(Starters::Owner(owner.clone()).sddl(), task_sddl(&owner));
    assert_eq!(Starters::Owner(owner.clone()).owner(), Some(&owner));
    let install = Starters::Interactive { denied: None };
    assert_eq!(
        install.sddl(),
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;IU)"
    );
    assert_eq!(install.owner(), None);
    // After the first set-up: the group's deny comes first, so it wins over IU's grant.
    let denied = Starters::Interactive {
        denied: Some(group),
    };
    assert_eq!(
        denied.sddl(),
        "O:BAD:P(D;;FRFX;;;S-1-5-21-1-2-3-1050)(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;IU)"
    );
    let sddl = denied.sddl();
    let aces: Vec<&str> = sddl.split('(').skip(1).collect();
    assert!(aces[0].starts_with("D;;FRFX;;;"), "{aces:?}");
    for ace in &aces[3..] {
        for wider in ["FA", "FW", "GA", "GW", "WD", "WO", "SD"] {
            assert!(!ace.contains(wider), "{wider}: {ace}");
        }
    }
}

#[test]
fn the_folder_has_its_own_descriptor_whatever_task_is_registered() {
    // Not any task's: a folder given the stop task's descriptor (one owner) would shut every
    // other interactive user out of the set-up task inside it.
    assert_eq!(FOLDER_SDDL, "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;IU)");
    let owner = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    assert_ne!(FOLDER_SDDL, task_sddl(&owner));
    assert_ne!(FOLDER_SDDL, Starters::Interactive { denied: None }.sddl());
    // Read alone for IU: no one but SYSTEM and Administrators adds or changes a task in it.
    let aces: Vec<&str> = FOLDER_SDDL.split('(').skip(1).collect();
    assert_eq!(aces.len(), 3);
    assert_eq!(aces[2], "A;;FR;;;IU)");
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
            Teardown::Task(SystemTask::SetUp),
            Teardown::Task(SystemTask::Refresh),
            Teardown::Task(SystemTask::Stop),
            Teardown::Staging,
            Teardown::AdminFolder,
        ]
    );
    for task in [SystemTask::SetUp, SystemTask::Refresh, SystemTask::Stop] {
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
fn the_action_names_the_admin_copy_in_quotes() {
    let exe = Path::new(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe");
    assert_eq!(
        action_path(exe),
        r#""C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe""#
    );
}

#[test]
fn a_missing_task_or_folder_is_not_registered_whichever_code_windows_gives() {
    // ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND as HRESULTs: either leaves the prompt.
    assert!(not_registered(0x8007_0002));
    assert!(not_registered(0x8007_0003));
    // Access denied, and a bare Win32 code, are failures, not a missing task.
    assert!(!not_registered(0x8007_0005));
    assert!(!not_registered(2));
}

/// A change to a read-back definition, and what it is.
type Change = (&'static str, fn(&mut Registered));

/// `spec` for `starters` as a fresh `register_task` reads back, its action `path`.
fn fresh(spec: SystemTask, starters: &Starters, path: &str) -> Registered {
    Registered {
        path: path.to_owned(),
        arguments: spec.argument().to_owned(),
        actions: 1,
        user: "SYSTEM".to_owned(),
        service_highest: true,
        instances: INSTANCES_IGNORE_NEW,
        limit: spec.execution_limit().to_owned(),
        working_dir: String::new(),
        enabled: true,
        hidden: false,
        triggers: i32::from(spec.at_logon() && starters.owner().is_some()),
        sddl: starters.sddl(),
    }
}

#[test]
fn a_fresh_registration_is_current_and_any_change_to_its_definition_is_not() {
    let path = r#""C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe""#;
    let owner = Sid::parse("S-1-12-1-3263492174-1177263218-2391161522-1513380577").unwrap();
    for starters in [
        Starters::Owner(owner),
        Starters::Interactive { denied: None },
    ] {
        let sddl = starters.sddl();
        for spec in [SystemTask::Refresh, SystemTask::Stop, SystemTask::SetUp] {
            let current = fresh(spec, &starters, path);
            // Read back as it was written: no registration again on every set-up.
            assert!(
                spec.registered_as(&current, &starters, path, &sddl),
                "{spec:?}"
            );
            let changes: [Change; 13] = [
                ("parallel", |found| found.instances = 0),
                ("queue", |found| found.instances = INSTANCES_QUEUE),
                ("stop existing", |found| found.instances = 3),
                ("disabled", |found| found.enabled = false),
                ("hidden", |found| found.hidden = true),
                ("a working folder", |found| {
                    found.working_dir = r"C:\Temp".to_owned();
                }),
                ("another program", |found| {
                    found.path = r#""C:\x.exe""#.to_owned();
                }),
                ("another argument", |found| {
                    found.arguments = "--other".to_owned();
                }),
                ("two actions", |found| found.actions = 2),
                ("another user", |found| {
                    found.user = "S-1-5-21-1-2-3-1001".to_owned();
                }),
                ("not highest", |found| found.service_highest = false),
                ("another limit", |found| found.limit = "PT72H".to_owned()),
                ("another trigger", |found| found.triggers += 1),
            ];
            for (change, apply) in changes {
                let mut found = current.clone();
                apply(&mut found);
                assert!(
                    !spec.registered_as(&found, &starters, path, &sddl),
                    "{spec:?}: {change}"
                );
            }
        }
    }
    assert_eq!((INSTANCES_QUEUE, INSTANCES_IGNORE_NEW), (1, 2));
}

#[test]
fn a_task_descriptor_reads_back_current_with_only_the_task_schedulers_own_changes() {
    // As CI run 42504256341 registered the set-up task and read it back.
    let given = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)";
    let stored = "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)";
    assert_eq!(TASK_SCHEDULER_ADDS, "(A;;FR;;;SY)");
    assert!(
        !sddl_matches(stored, given),
        "the plain compare tells them apart"
    );
    for current in [
        given,
        stored,
        "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)",
        "O:BAD:PAI(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)",
    ] {
        assert!(task_sddl_matches(current, given), "{current}");
    }
    let changed = [
        (
            "a write grant added",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FW;;;IU)",
        ),
        (
            "a write grant after SYSTEM's read",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)(A;;FW;;;IU)",
        ),
        (
            "a grant to everyone in its place",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;WD)",
        ),
        (
            "SYSTEM's read twice",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)(A;;FR;;;SY)",
        ),
        (
            "a broader mask",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;IU)(A;;FR;;;SY)",
        ),
        (
            "an entry missing",
            "O:BAD:(A;;FA;;;SY)(A;;0x1200a9;;;IU)(A;;FR;;;SY)",
        ),
        (
            "reordered",
            "O:BAD:(A;;FA;;;BA)(A;;FA;;;SY)(A;;0x1200a9;;;IU)(A;;FR;;;SY)",
        ),
        (
            "inherited",
            "O:BAD:AI(A;;FA;;;SY)(A;;FA;;;BA)(A;ID;0x1200a9;;;IU)(A;;FR;;;SY)",
        ),
        (
            "an added entry first",
            "O:BAD:(A;;FR;;;SY)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)",
        ),
        (
            "another owner",
            "O:S-1-5-21-1-2-3-1001D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)",
        ),
        ("a null DACL", "O:BAD:NO_ACCESS_CONTROL"),
        ("no DACL", "O:BA"),
        (
            "a SACL after it",
            "O:BAD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)S:(ML;;NW;;;LW)",
        ),
    ];
    for (change, found) in changed {
        assert!(!task_sddl_matches(found, given), "{change}");
    }
    // The pool's group refused first: its deny must stay, in front.
    let denied =
        "O:BAD:P(D;;0x1200a9;;;S-1-5-21-1-2-3-1050)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)";
    let stored = "O:BAD:(D;;0x1200a9;;;S-1-5-21-1-2-3-1050)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;IU)(A;;FR;;;SY)";
    assert!(task_sddl_matches(stored, denied));
    assert!(!task_sddl_matches(
        stored
            .replacen("(D;;0x1200a9;;;S-1-5-21-1-2-3-1050)", "", 1)
            .as_str(),
        denied
    ));
}

#[test]
fn a_set_up_task_stored_by_the_task_scheduler_is_current_and_a_widened_one_is_not() {
    let path = r#""C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe""#;
    let starters = Starters::Interactive { denied: None };
    let sddl = starters.sddl();
    let spec = SystemTask::SetUp;
    let mut found = fresh(spec, &starters, path);
    found.sddl = format!("{}{TASK_SCHEDULER_ADDS}", sddl.replacen("D:P", "D:", 1));
    assert!(
        spec.registered_as(&found, &starters, path, &sddl),
        "{}",
        found.sddl
    );
    found.sddl = format!("{}(A;;FW;;;IU)", sddl.replacen("D:P", "D:", 1));
    assert!(
        !spec.registered_as(&found, &starters, path, &sddl),
        "{}",
        found.sddl
    );
}

#[test]
fn every_task_is_disabled_then_stopped_then_deleted() {
    assert_eq!(RETIRE, [Retire::Disable, Retire::Stop, Retire::Delete]);
    // The fake task: each step taken, in order, and the one that fails.
    let take_down = |failing: Option<Retire>| {
        let mut taken = Vec::new();
        let done = retire(|step| {
            taken.push(step);
            if Some(step) == failing {
                Err(step)
            } else {
                Ok(())
            }
        });
        (taken, done)
    };
    assert_eq!(
        take_down(None),
        (vec![Retire::Disable, Retire::Stop, Retire::Delete], Ok(()))
    );
    // A task that can't be disabled is neither stopped nor deleted: the removal fails there.
    assert_eq!(
        take_down(Some(Retire::Disable)),
        (vec![Retire::Disable], Err(Retire::Disable))
    );
    assert_eq!(
        take_down(Some(Retire::Stop)),
        (vec![Retire::Disable, Retire::Stop], Err(Retire::Stop))
    );
}
