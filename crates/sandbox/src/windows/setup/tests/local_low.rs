//! The owners of the folders the shared-write denies protect (S15 `PR4h`, item 11): before any
//! deny is written, a pool user's `LocalLow` that isn't `SYSTEM`'s, `Administrators`' or the
//! set-up's own goes to `Administrators`; one that won't fails at `Step::Owners`.

use super::fake::ADMINISTRATORS;
use super::*;

/// The position of the first call `line` in `machine`'s calls.
fn at(machine: &FakeMachine, line: &str) -> usize {
    machine.calls.iter().position(|c| c == line).unwrap()
}

#[test]
fn a_local_low_a_pool_user_owns_goes_to_administrators_before_the_denies() {
    let mut machine = FakeMachine::new("setup-local-low-owned");
    machine
        .self_owned_local_low
        .insert("ClauseoSb02".to_owned());
    let mut acl = FakeAcl::of(&machine);
    let report = set_up_fresh(&mut machine, &mut acl);
    let local_low = local_low(&machine, "ClauseoSb02");
    assert_eq!(
        machine.owners.get(&local_low),
        Some(&Sid::parse(ADMINISTRATORS).unwrap())
    );
    assert!(at(&machine, "give_to_admins ClauseoSb02") < at(&machine, "program_data"));
    // Read again after the hand-over.
    let reads = machine
        .calls
        .iter()
        .filter(|c| *c == "owner_of ClauseoSb02")
        .count();
    assert_eq!(reads, 2);
    // The denies were written on it after.
    assert!(
        acl.adds
            .iter()
            .any(|(path, _)| path.starts_with(&local_low))
    );
    assert_eq!(report.made.len(), 4);
}

#[test]
fn a_local_low_system_owns_is_left_alone() {
    let mut machine = FakeMachine::new("setup-local-low-system");
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c.starts_with("give_to_admins"))
    );
    assert!(machine.owners.is_empty(), "every LocalLow still SYSTEM's");
}

#[test]
fn an_owner_that_cant_be_replaced_fails_at_its_own_step_with_no_deny_written() {
    let mut machine = FakeMachine::new("setup-local-low-refused");
    machine
        .self_owned_local_low
        .insert("ClauseoSb03".to_owned());
    machine.fail("give_to_admins ClauseoSb03");
    let mut acl = FakeAcl::of(&machine);
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (Step::Owners, SetupExit::Failed)
    );
    assert!(failure.cause.contains("LocalLow"), "{}", failure.cause);
    assert!(failure.rolled_back, "{}", failure.cause);
    assert_eq!(acl.adds, Vec::<(PathBuf, bool)>::new(), "no deny written");
    assert!(!machine.calls.iter().any(|c| c == "program_data"));
}

#[test]
fn an_owner_still_a_pool_user_after_the_hand_over_fails() {
    let mut machine = FakeMachine::new("setup-local-low-sticks");
    machine
        .self_owned_local_low
        .insert("ClauseoSb01".to_owned());
    machine.stuck_owners.insert("ClauseoSb01".to_owned());
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Owners);
    assert!(
        failure.cause.contains("still owned by"),
        "{}",
        failure.cause
    );
}

#[test]
fn a_re_run_hands_over_a_kept_users_local_low_too() {
    let mut machine = FakeMachine::new("setup-local-low-rerun");
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    let local_low = local_low(&machine, "ClauseoSb04");
    let pool_user = machine.state.accounts["ClauseoSb04"].sid.clone();
    machine.owners.insert(local_low.clone(), pool_user);
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap();
    assert_eq!(
        machine.owners.get(&local_low),
        Some(&Sid::parse(ADMINISTRATORS).unwrap())
    );
}

#[test]
fn device_sync_owned_by_a_pool_user_fails_and_is_never_handed_over() {
    let mut machine = FakeMachine::new("setup-device-sync-owned");
    let device_sync = machine.device_sync();
    // The group the run makes is the first SID the fake gives out.
    machine.owners.insert(device_sync.clone(), fake::sid(1000));
    let mut acl = FakeAcl::of(&machine);
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Owners);
    assert!(failure.cause.contains("DeviceSync"), "{}", failure.cause);
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c == "give_to_admins DeviceSync")
    );
    assert!(
        !acl.adds
            .iter()
            .any(|(path, _)| path.starts_with(&device_sync))
    );
}

#[test]
fn device_sync_windows_owns_is_only_read() {
    let mut machine = FakeMachine::new("setup-device-sync-windows");
    let trusted_installer =
        Sid::parse("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464").unwrap();
    machine
        .owners
        .insert(machine.device_sync(), trusted_installer);
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert!(machine.calls.iter().any(|c| c == "owner_of DeviceSync"));
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c == "give_to_admins DeviceSync")
    );
}

/// A profile with no `LocalLow`: the owner check skips it, and step 7's deny fails on it as
/// missing, at `SharedWrites`, not `Owners`.
#[test]
fn a_missing_local_low_fails_at_the_shared_writes_not_the_owners() {
    let mut machine = FakeMachine::new("setup-local-low-missing");
    machine.no_local_low.insert("ClauseoSb02".to_owned());
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::SharedWrites, "{}", failure.cause);
    assert!(failure.cause.contains("ClauseoSb02"), "{}", failure.cause);
    assert!(machine.calls.iter().any(|c| c == "exists ClauseoSb02"));
    assert!(
        !machine.calls.iter().any(|c| c == "owner_of ClauseoSb02"),
        "{:?}",
        machine.calls
    );
}
