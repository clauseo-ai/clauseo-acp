//! Whose the set-up's writes are (S15 `PR4h`, split §9): every call on the owner's files under
//! `<data_dir>\sandbox\` (the marker, the ledger, the open sessions, `secrets.bin`) and every
//! write of the fence runs as the owner, through `Machine::as_owner`; the machine-wide writes
//! (the shared folders' denies) run as the elevated process itself. A request for any user but
//! the machine's owner is refused before anything is touched.

use super::fake::OTHER_OWNER;
use super::*;

/// No call on the owner's files was made outside `as_owner`, and the thread acts as itself
/// again.
fn all_as_owner(machine: &FakeMachine) {
    assert_eq!(machine.outside_owner, Vec::<String>::new());
    assert!(
        !machine.acting.get(),
        "the thread acts as itself after the set-up"
    );
}

#[test]
fn a_fresh_set_up_writes_secrets_and_the_marker_as_the_owner() {
    let mut machine = FakeMachine::new("setup-owner-fresh");
    let mut acl = FakeAcl::of(&machine);
    set_up_fresh(&mut machine, &mut acl);
    all_as_owner(&machine);
    assert!(machine.calls.iter().any(|c| c == "write_marker"));
    assert!(
        machine
            .calls
            .iter()
            .any(|c| c.starts_with("protect_for_owner"))
    );
    // The shared folders' denies are machine-wide: the elevated process writes them itself.
    assert_ne!(acl.adds, []);
    assert!(
        acl.adds.iter().all(|(_, as_owner)| !as_owner),
        "{:?}",
        acl.adds
    );
}

#[test]
fn an_extend_fences_the_matters_as_the_owner_and_the_shared_folders_as_itself() {
    let mut machine = FakeMachine::new("setup-owner-extend");
    let mut acl = FakeAcl::of(&machine);
    set_up_fresh(&mut machine, &mut acl);
    let matters = two_matters(&machine);
    acl.adds.clear();
    let req = request(&machine, 4, matters.clone());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    all_as_owner(&machine);
    assert!(report.fenced.unwrap().written > 0);

    let mut folders: Vec<PathBuf> = Vec::new();
    for matter in &matters {
        folders.push(dunce::canonicalize(&matter.folders.root).unwrap());
        folders.push(dunce::canonicalize(&matter.folders.state).unwrap());
    }
    let (fence, shared): (Vec<_>, Vec<_>) = acl
        .adds
        .iter()
        .partition(|(path, _)| folders.iter().any(|folder| path.starts_with(folder)));
    assert!(!fence.is_empty() && !shared.is_empty(), "{:?}", acl.adds);
    assert!(fence.iter().all(|(_, as_owner)| *as_owner), "{fence:?}");
    assert!(shared.iter().all(|(_, as_owner)| !as_owner), "{shared:?}");
}

#[test]
fn a_re_run_reads_the_marker_the_ledger_and_the_sessions_as_the_owner() {
    let mut machine = FakeMachine::new("setup-owner-rerun");
    let mut acl = FakeAcl::of(&machine);
    set_up_fresh(&mut machine, &mut acl);
    machine.state.accounts.remove("ClauseoSb02");
    let req = request(&machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.made.len(), 1);
    for call in ["read_marker", "ledger", "sessions_open"] {
        assert!(machine.calls.iter().any(|c| c == call), "{call}");
    }
    all_as_owner(&machine);
}

#[test]
fn a_rolled_back_re_run_writes_secrets_back_as_the_owner() {
    let mut machine = FakeMachine::new("setup-owner-rollback");
    let mut acl = FakeAcl::of(&machine);
    set_up_fresh(&mut machine, &mut acl);
    machine.fail("write_marker");
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Marker);
    let protects = machine
        .calls
        .iter()
        .filter(|c| c.starts_with("protect_for_owner"))
        .count();
    assert_eq!(protects, 3, "the first run, this run, and the rollback's");
    all_as_owner(&machine);
}

#[test]
fn a_request_for_another_user_is_refused_before_the_lock() {
    let mut machine = FakeMachine::new("setup-owner-other");
    let mut req = request(&machine, 0, Vec::new());
    req.owner = Sid::parse(OTHER_OWNER).unwrap();
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Owner);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back);
    assert!(failure.cause.contains(OTHER_OWNER), "{}", failure.cause);
    assert!(failure.cause.contains(fake::OWNER), "{}", failure.cause);
    assert_eq!(machine.calls, Vec::<String>::new());

    // The machine's owner decides, whatever the request says: a machine acting for the other
    // user refuses the request for the owner.
    machine.owner = Sid::parse(OTHER_OWNER).unwrap();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Owner);
    assert_eq!(machine.calls, Vec::<String>::new());
}

#[test]
fn an_owner_that_cant_be_impersonated_fails_the_set_up_and_rolls_it_back() {
    let mut machine = FakeMachine::new("setup-owner-refused");
    machine.refuse_owner = true;
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Secrets);
    assert!(failure.cause.contains("impersonating"), "{}", failure.cause);
    assert!(failure.rolled_back, "{}", failure.cause);
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c.starts_with("protect_for_owner"))
    );
    assert!(!machine.calls.iter().any(|c| c == "write_marker"));
    let users = machine
        .state
        .accounts
        .keys()
        .filter(|n| n.starts_with("ClauseoSb"));
    assert_eq!(users.count(), 0, "every user made is gone again");
    assert_eq!(machine.state.marker, None);
}
