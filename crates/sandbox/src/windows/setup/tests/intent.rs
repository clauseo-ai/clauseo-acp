//! What a run ended part way leaves (`intent.rs`), against the fake: a kill after each change,
//! then `--remove` and a re-run, each leaving nothing behind; an account with a pool name that no
//! intent names is never deleted; a torn first record; another user's partial set-up.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::fake::{Account, FakeAcl, FakeMachine, OTHER_OWNER};
use super::remove::assert_all_gone;
use super::remove_fake::Remover;
use super::*;

/// A fresh set-up killed before its change number `after + 1` (none undone), and its writer.
fn killed(after: usize) -> Option<(FakeMachine, FakeAcl)> {
    let mut machine = FakeMachine::new("intent-killed");
    let mut acl = FakeAcl::default();
    machine.life.kill_after = Some(after);
    let req = request(&mut machine, 0, Vec::new());
    let run = catch_unwind(AssertUnwindSafe(|| set_up(&mut machine, &mut acl, &req)));
    machine.life.kill_after = None;
    // The process is gone, and its impersonation with it.
    machine.acting.set(false);
    run.is_err().then_some((machine, acl))
}

/// `--remove` by `invoker` (per machine when `per_machine`) over `machine`, then nothing left.
fn remove_all(machine: FakeMachine, acl: FakeAcl, invoker: Option<Sid>, per_machine: bool) {
    let remover = Remover::new(machine, invoker);
    let mut acl = remover.acl(acl);
    let mut remover = remover;
    let removed = remove(&mut remover, &mut acl, per_machine);
    assert!(removed.is_ok(), "{removed:?}");
    assert_all_gone(&remover, &acl);
    let state = &remover.m.state;
    assert_eq!(state.partial, None);
    assert!(!state.pending_group && state.pending_users.is_empty());
    assert!(state.remote_denied.is_empty());
}

/// A set-up that succeeds over `machine`, then every account in the pool's group recorded and no
/// intent left.
fn rerun(machine: &mut FakeMachine, acl: &mut FakeAcl, case: &str) {
    let req = request(machine, 0, Vec::new());
    let report = set_up(machine, acl, &req);
    assert!(report.is_ok(), "{case}: {report:?}");
    let state = &machine.state;
    let record = state.record.clone().unwrap();
    assert_eq!(state.group.as_ref(), Some(&record.group), "{case}");
    for (name, account) in &state.accounts {
        if account.groups.contains(&record.group) {
            assert!(
                record.pool.contains(&account.sid),
                "{case}: {name} recorded"
            );
        }
    }
    assert!(
        !state.pending_group && state.pending_users.is_empty(),
        "{case}"
    );
    assert_eq!(state.partial, None, "{case}");
}

#[test]
fn a_kill_after_each_change_leaves_what_remove_and_a_re_run_both_finish() {
    let mut after = 0;
    while let Some((machine, acl)) = killed(after) {
        let case = format!("killed after {after}");
        remove_all(machine, acl, Some(owner()), false);
        let (machine, acl) = killed(after).unwrap();
        remove_all(machine, acl, Some(Sid::parse(OTHER_OWNER).unwrap()), true);
        let (mut machine, mut acl) = killed(after).unwrap();
        rerun(&mut machine, &mut acl, &case);
        remove_all(machine, acl, Some(owner()), false);
        after += 1;
    }
    // Every change of a fresh run was a place to be killed.
    let mut whole = FakeMachine::new("intent-whole");
    set_up_fresh(&mut whole, &mut FakeAcl::default());
    assert_eq!(after, whole.life.changes);
}

/// An account named `name` that set-up didn't make: no comment of ours, in no group of ours,
/// unless `marked`.
fn outside_account(machine: &mut FakeMachine, name: &str, rid: u32, marked: bool) -> Sid {
    let sid = sid(rid);
    let account = Account {
        sid: sid.clone(),
        password: "theirs".to_owned(),
        groups: std::collections::BTreeSet::default(),
        hidden: false,
        marked,
    };
    machine.state.accounts.insert(name.to_owned(), account);
    sid
}

#[test]
fn an_account_with_a_pool_name_and_no_intent_is_never_deleted() {
    // Ours by its comment even, but no intent names it: skipped by the set-up (D-B5) and left by
    // the removal.
    let mut machine = FakeMachine::new("intent-unnamed");
    let mut acl = FakeAcl::default();
    let stranger = outside_account(&mut machine, "ClauseoSb01", 900, true);
    rerun(&mut machine, &mut acl, "unnamed");
    assert_eq!(machine.state.accounts["ClauseoSb01"].sid, stranger);
    let remover = Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    let mut remover = remover;
    remove(&mut remover, &mut acl, false).unwrap();
    assert_eq!(remover.m.state.accounts["ClauseoSb01"].sid, stranger);
    assert!(
        !remover
            .lines()
            .contains(&"delete_user ClauseoSb01".to_owned())
    );
}

#[test]
fn an_intent_never_deletes_an_account_that_isnt_ours() {
    // The intent names ClauseoSb03, but the account by that name carries no comment of ours and
    // is in no group of ours: it stays, and the intent is cleared.
    let mut machine = FakeMachine::new("intent-not-ours");
    let mut acl = FakeAcl::default();
    let theirs = outside_account(&mut machine, "ClauseoSb03", 900, false);
    machine.state.partial = Some(crate::windows::setup::record_values::RawRecord {
        owner: Some(OWNER.to_owned()),
        ..Default::default()
    });
    machine.state.pending_users = vec!["ClauseoSb03".to_owned()];
    rerun(&mut machine, &mut acl, "not ours");
    assert_eq!(machine.state.accounts["ClauseoSb03"].sid, theirs);
    // The same intent naming an account that is ours (a set-up killed after making it): gone.
    let mut machine = FakeMachine::new("intent-ours");
    outside_account(&mut machine, "ClauseoSb03", 900, true);
    machine.state.partial = Some(crate::windows::setup::record_values::RawRecord {
        owner: Some(OWNER.to_owned()),
        ..Default::default()
    });
    machine.state.pending_users = vec!["ClauseoSb03".to_owned(), "Administrator".to_owned()];
    let mut acl = FakeAcl::default();
    rerun(&mut machine, &mut acl, "ours");
    assert_ne!(machine.state.accounts["ClauseoSb03"].sid, sid(900));
    assert!(
        machine
            .calls
            .contains(&"delete_user ClauseoSb03".to_owned())
    );
    // A name outside the pool is never acted on, whatever the intent says.
    assert!(!machine.calls.iter().any(|c| c.contains("Administrator")));
}

/// The change number of the first record write's second value: a kill there tears it.
fn torn_first_record() -> usize {
    let mut whole = FakeMachine::new("intent-torn-whole");
    set_up_fresh(&mut whole, &mut FakeAcl::default());
    let first = whole
        .life
        .trail
        .iter()
        .position(|change| change == "write_record's value")
        .unwrap();
    first + 1
}

#[test]
fn a_torn_first_record_is_redone_by_set_up_and_cleaned_by_remove() {
    let at = torn_first_record();
    let (machine, acl) = killed(at).unwrap();
    // Torn: `GroupSid` only, over the intent's `OwnerSid`; no whole record.
    assert_eq!(machine.state.record, None);
    let partial = machine.state.partial.clone().unwrap();
    assert!(partial.group.is_some() && partial.data_dir.is_none());
    assert!(partial.owner.is_some());
    remove_all(machine, acl, Some(owner()), false);
    let (machine, acl) = killed(at).unwrap();
    remove_all(machine, acl, None, true);
    let (mut machine, mut acl) = killed(at).unwrap();
    rerun(&mut machine, &mut acl, "torn");
}

#[test]
fn another_users_partial_set_up_is_theirs() {
    let at = torn_first_record();
    let (mut machine, mut acl) = killed(at).unwrap();
    let before = machine.state.clone();
    // Another user's set-up refuses, as for a whole record (D5), and changes nothing.
    machine.owner = Sid::parse(OTHER_OWNER).unwrap();
    let req = SetupRequest {
        owner: machine.owner.clone(),
        data_dir: machine.data_dir(),
        add: 0,
    };
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.exit, SetupExit::OtherOwner);
    assert_eq!(machine.state, before);
    // Their per-user removal touches nothing; a per-machine one cleans it.
    machine.owner = owner();
    let remover = Remover::new(machine, Some(Sid::parse(OTHER_OWNER).unwrap()));
    let mut racl = remover.acl(acl);
    let mut remover = remover;
    assert_eq!(
        remove(&mut remover, &mut racl, false),
        Ok(Removed::NotOwner)
    );
    assert_eq!(remover.m.state, before);
    assert!(matches!(
        remove(&mut remover, &mut racl, true),
        Ok(Removed::All(_))
    ));
    assert_all_gone(&remover, &racl);
}
