//! `--adopt`'s lock denies the pool's group once the record names one (`GroupSid`), and locks
//! with no deny on a first install, before set-up has made the group.

use super::*;

const GROUP: &str = "S-1-5-21-1-2-3-1100";

/// The admin folder as the fake's lock line names it.
fn sandbox() -> String {
    admin_copy::admin_dir(Path::new(r"C:\Program Files"))
        .display()
        .to_string()
}

#[test]
fn a_first_install_locks_with_no_deny_and_a_later_adopt_denies_the_recorded_group() {
    // No record yet: the set-up this `--adopt` runs makes the group and locks again.
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert_eq!(fake.changes[0], format!("lock {}", sandbox()));
    // A repair or upgrade over a set-up pool: the record's group is denied.
    for target in [user(), Target::Machine] {
        let mut fake = Fake::new();
        fake.owner = Some(lawyer());
        fake.group = Some(Sid::parse(GROUP).unwrap());
        let install = if target == Target::Machine {
            MACHINE
        } else {
            INSTALL
        };
        let exit = adopt(&mut fake, Path::new(install), &target);
        assert_eq!(exit, SetupExit::Done, "{target:?}");
        assert_eq!(fake.changes[0], format!("lock {} deny {GROUP}", sandbox()));
    }
}
