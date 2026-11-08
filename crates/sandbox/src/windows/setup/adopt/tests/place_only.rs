//! `--adopt --place-only`: the copy placed, locked down and recorded, and the set-up task
//! registered for the lawyer, nothing else (no refresh or stop task, no set-up), with plain
//! `--adopt`'s exits; plain `--adopt` still registers the other two and sets up.

use super::*;

/// The lawyer's `--adopt --place-only`.
fn place_only() -> Target {
    Target::User {
        owner_sid: lawyer(),
        data_dir: PathBuf::from(DATA),
        place_only: true,
    }
}

/// The lock-down's change, as the fake writes it on this OS.
fn locked() -> String {
    let dir = admin_copy::admin_dir(Path::new(r"C:\Program Files"));
    format!("lock {}", dir.display())
}

#[test]
fn place_only_locks_down_registers_the_set_up_task_and_records_with_no_set_up() {
    let mut fake = Fake::new();
    // A refresh task that would fail, and a set-up that would: neither is reached.
    fake.failing_task = Some(SystemTask::Refresh);
    fake.set_up_exit = SetupExit::Failed;
    let target = place_only();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &target),
        SetupExit::Done
    );
    let version = admin_copy::APP_VERSION;
    assert_eq!(
        fake.changes,
        [
            locked(),
            format!("task SandboxSetUp {LAWYER}"),
            format!("record {HASH} {INSTALL} {version} {LAWYER}"),
        ]
    );
    assert!(!fake.changes.iter().any(|c| c.starts_with("set-up ")));
    assert!(fake.reports.is_empty(), "{:?}", fake.reports);
    assert!(!fake.held, "the lock is given back");
    // The launcher's check then finds the copy, so the app's first run uses it to set up.
    let mut copy = admin_copy::fake::Fake::ready();
    copy.recorded = fake.recorded.clone();
    assert!(admin_copy::check(&copy).is_ok());
}

#[test]
fn plain_adopt_still_registers_both_tasks_and_sets_up() {
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    let version = admin_copy::APP_VERSION;
    assert_eq!(
        fake.changes,
        [
            locked(),
            format!("task SandboxRefresh {LAWYER}"),
            format!("task SandboxStop {LAWYER}"),
            format!("record {HASH} {INSTALL} {version} {LAWYER}"),
            format!("set-up {DATA}"),
        ]
    );
    // Its set-up's exit is still its own, where --place-only's is `Done`.
    for (target, exit) in [
        (user(), SetupExit::LockHeld),
        (place_only(), SetupExit::Done),
    ] {
        let mut fake = Fake::new();
        fake.set_up_exit = SetupExit::LockHeld;
        assert_eq!(
            adopt(&mut fake, Path::new(INSTALL), &target),
            exit,
            "{target:?}"
        );
    }
}

#[test]
fn place_only_refuses_as_plain_adopt_does_and_changes_nothing() {
    type Arrange = fn(&mut Fake) -> &'static str;
    let cases: [(Arrange, SetupExit); 8] = [
        (|_| INSTALL, SetupExit::Done),
        (
            |f| {
                f.token = (false, false);
                INSTALL
            },
            SetupExit::NotElevated,
        ),
        (
            |f| {
                f.image = PathBuf::from(BESIDE_APP);
                INSTALL
            },
            SetupExit::NotAdminCopy,
        ),
        (
            |f| {
                f.lock_busy = true;
                INSTALL
            },
            SetupExit::LockHeld,
        ),
        (
            |f| {
                f.owner = Some(Sid::parse(OTHER).unwrap());
                INSTALL
            },
            SetupExit::OtherOwner,
        ),
        (
            |f| {
                f.version = Some("999.0.0".to_owned());
                INSTALL
            },
            SetupExit::Downgrade,
        ),
        (
            |_| r"C:\Users\other\AppData\Local\Clauseo",
            SetupExit::Usage,
        ),
        (
            |f| {
                f.fail = Some(AdoptStep::LockDown);
                INSTALL
            },
            SetupExit::Failed,
        ),
    ];
    for (arrange, exit) in cases {
        let mut plain = Fake::new();
        let install = arrange(&mut plain);
        assert_eq!(adopt(&mut plain, Path::new(install), &user()), exit);
        let mut fake = Fake::new();
        let install = arrange(&mut fake);
        assert_eq!(
            adopt(&mut fake, Path::new(install), &place_only()),
            exit,
            "{exit:?}"
        );
        if exit != SetupExit::Done {
            assert!(fake.changes.is_empty(), "{exit:?}: {:?}", fake.changes);
            assert!(fake.recorded.is_none(), "{exit:?}");
        }
        // The same failure reported, for the result file in --data-dir.
        assert_eq!(fake.reports, plain.reports, "{exit:?}");
    }
}
