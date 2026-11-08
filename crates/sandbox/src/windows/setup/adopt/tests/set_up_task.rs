//! The set-up task `\Clauseo\SandboxSetUp` at `--adopt`: `--place-only` registers it for the
//! lawyer alone, a per-machine `--adopt` for every interactive user with the recorded pool group
//! refused, plain per-user `--adopt` not at all; a failed registration is exit 3 at step
//! `set_up_task` with no record value written.

use super::*;

/// The lawyer's `--adopt --place-only`.
fn place_only() -> Target {
    Target::User {
        owner_sid: lawyer(),
        data_dir: PathBuf::from(DATA),
        place_only: true,
    }
}

#[test]
fn a_set_up_task_that_cant_be_registered_exits_3_with_no_record_value() {
    for (target, install) in [(place_only(), INSTALL), (Target::Machine, MACHINE)] {
        let mut fake = Fake::new();
        fake.failing_task = Some(SystemTask::SetUp);
        assert_eq!(
            adopt(&mut fake, Path::new(install), &target),
            SetupExit::Failed,
            "{target:?}"
        );
        assert_eq!(SetupExit::Failed.as_code(), 3);
        assert!(fake.recorded.is_none(), "{target:?}: no value written");
        assert!(
            !fake.changes.iter().any(|c| c.starts_with("record ")),
            "{:?}",
            fake.changes
        );
        assert_eq!(fake.reports.len(), 1, "{target:?}");
        assert_eq!(fake.reports[0].step, AdoptStep::Task(SystemTask::SetUp));
        assert_eq!(fake.reports[0].step.name(), "set_up_task");
        let line = fake.reports[0].line();
        assert!(
            line.starts_with("--adopt failed (exit 3) at step set_up_task: "),
            "{line}"
        );
        assert!(line.ends_with("HRESULT 0x80041318"), "{line}");
        // The result file (the per-user report) names the same step.
        let result = fake.reports[0].result();
        assert_eq!(result.exit, 3);
        assert_eq!(result.step.as_deref(), Some("set_up_task"));
        assert!(!fake.held, "the lock is given back");
    }
}

#[test]
fn a_per_machine_adopt_refuses_the_recorded_pool_group_the_set_up_task() {
    let group = Sid::parse("S-1-5-21-1-2-3-1050").unwrap();
    let mut fake = Fake::new();
    fake.group = Some(group.clone());
    fake.owner = Some(lawyer());
    fake.copy_owner = Some(CopyOwner::Machine);
    assert_eq!(
        adopt(&mut fake, Path::new(MACHINE), &Target::Machine),
        SetupExit::Done
    );
    // A reinstall over a set-up pool keeps the deny the first set-up added.
    assert_eq!(
        fake.changes[1],
        format!("task SandboxSetUp interactive but {group}")
    );
    // No group recorded yet: interactive users alone; the first set-up adds the deny.
    let mut fake = Fake::new();
    adopt(&mut fake, Path::new(MACHINE), &Target::Machine);
    assert_eq!(fake.changes[1], "task SandboxSetUp interactive");
}

#[test]
fn only_place_only_and_per_machine_register_the_set_up_task() {
    let set_up_task = |fake: &Fake| {
        fake.changes
            .iter()
            .filter(|c| c.starts_with("task SandboxSetUp"))
            .count()
    };
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert_eq!(set_up_task(&fake), 0, "plain per-user --adopt");
    let mut fake = Fake::new();
    adopt(&mut fake, Path::new(INSTALL), &place_only());
    assert_eq!(set_up_task(&fake), 1, "--place-only");
    // Registered after the lock-down, before the values, as the other tasks are.
    assert!(fake.changes[0].starts_with("lock "), "{:?}", fake.changes);
    assert!(fake.changes[2].starts_with("record "), "{:?}", fake.changes);
    let mut fake = Fake::new();
    adopt(&mut fake, Path::new(MACHINE), &Target::Machine);
    assert_eq!(set_up_task(&fake), 1, "--per-machine");
}
