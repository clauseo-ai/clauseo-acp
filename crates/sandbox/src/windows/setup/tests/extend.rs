//! An extend (D-B5) and a re-run (D-B6) on a pool this owner set up.

use super::fake::rid;
use super::*;

/// `path` canonical, as the fence's grants name it.
fn canonical(path: &std::path::Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

#[test]
fn an_extend_adds_users_and_fences_every_registered_matter_with_every_new_sid() {
    let mut machine = FakeMachine::new("setup-extend");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let old: Vec<(String, String)> = (1..=4)
        .map(|n| {
            let name = format!("ClauseoSb0{n}");
            let password = machine.state.accounts[&name].password.clone();
            (name, password)
        })
        .collect();
    let matters = two_matters(&machine);
    let from = machine.calls.len();
    let req = request(&machine, 4, matters.clone());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();

    let mut expected: Vec<String> = ["lock", "read_record", "seclogon", "logon_right"]
        .map(str::to_owned)
        .into();
    expected.extend((1001..=1004).map(|r| format!("account_name {r}")));
    expected.push("group".to_owned());
    expected.extend((1001..=1004).map(|r| format!("profile_of {r}")));
    let mut recorded: Vec<String> = (1001..=1004).map(|r| r.to_string()).collect();
    for n in 5..=8u32 {
        let name = format!("ClauseoSb0{n}");
        recorded.push((1000 + n).to_string());
        expected.extend([
            format!("account_sid {name}"),
            format!("create_user {name}"),
            format!("add_to_group {name}"),
            format!("hide {name}"),
            format!("write_record {}", recorded.join(",")),
            format!("log_on_once {name}"),
        ]);
    }
    expected.extend(
        [
            "program_data",
            "ledger",
            "journal_volumes",
            "journal_size C:",
            "protect_for_owner 1005,1006,1007,1008",
            "write_marker",
            "unlock",
        ]
        .map(str::to_owned),
    );
    assert_eq!(machine.calls_since(from), expected);

    let made: Vec<Sid> = report.made.iter().map(|u| u.sid.clone()).collect();
    assert_eq!(made, (1005..=1008).map(sid).collect::<Vec<_>>());
    assert!(report.reset.is_empty(), "an extend resets no password");
    for (name, password) in &old {
        assert_eq!(&machine.state.accounts[name].password, password, "{name}");
        let sid = &machine.state.accounts[name].sid;
        assert_eq!(&machine.state.secrets[sid], password, "{name}'s blob kept");
    }
    assert_eq!(machine.state.secrets.len(), 8);
    assert_eq!(machine.state.record.clone().unwrap().pool.len(), 8);
    assert_eq!(machine.state.marker.clone().unwrap().pool.len(), 8);

    // Every registered matter's folders deny every new user; never the matter's own.
    let fenced = report.fenced.unwrap();
    assert!(fenced.written > 0);
    for matter in &matters {
        for folder in [&matter.folders.root, &matter.folders.state] {
            let folder = canonical(folder);
            for new in &made {
                assert!(
                    acl.has(&folder, new, Rights::DenyReadWrite),
                    "{} on {}",
                    rid(new),
                    folder.display()
                );
            }
            assert!(!acl.has(&folder, &matter.pool_user, Rights::DenyReadWrite));
        }
    }
    // The new users' `LocalLow` is closed too.
    for user in &report.made {
        assert!(acl.has(
            &local_low(&machine, &user.name),
            &sid(1000),
            Rights::DenyWrite
        ));
    }
}

#[test]
fn a_re_run_reuses_the_recorded_accounts_and_resets_their_passwords() {
    let mut machine = FakeMachine::new("setup-rerun");
    let mut acl = FakeAcl::default();
    let first = set_up_fresh(&mut machine, &mut acl);
    let old: Vec<String> = first
        .pool
        .iter()
        .map(|u| machine.state.accounts[&u.name].password.clone())
        .collect();
    machine.state.marker = None;
    let record = machine.state.record.clone();
    let from = machine.calls.len();
    let req = request(&machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();

    let mut expected: Vec<String> = ["lock", "read_record", "seclogon", "logon_right"]
        .map(str::to_owned)
        .into();
    expected.extend((1001..=1004).map(|r| format!("account_name {r}")));
    expected.extend(["sessions_open", "group"].map(str::to_owned));
    for n in 1..=4 {
        let name = format!("ClauseoSb0{n}");
        expected.extend(
            ["set_password", "add_to_group", "hide", "log_on_once"].map(|c| format!("{c} {name}")),
        );
    }
    expected.extend(
        [
            "program_data",
            "journal_volumes",
            "journal_size C:",
            "protect_for_owner 1001,1002,1003,1004",
            "write_marker",
            "unlock",
        ]
        .map(str::to_owned),
    );
    assert_eq!(machine.calls_since(from), expected);
    assert_eq!(report.pool, first.pool, "the same accounts and SIDs");
    assert_eq!(report.reset, first.pool);
    assert_eq!(report.made, []);
    assert!(!report.group_made);
    assert_eq!(machine.state.record, record);
    for (user, old) in first.pool.iter().zip(&old) {
        let now = &machine.state.accounts[&user.name].password;
        assert_ne!(now, old, "{}", user.name);
        assert_eq!(&machine.state.secrets[&user.sid], now, "{}", user.name);
    }
    assert!(machine.state.marker.is_some());
}

#[test]
fn a_re_run_refuses_while_a_session_is_open() {
    let mut machine = FakeMachine::new("setup-rerun-open");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.sessions_open = true;
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::Sessions, true));
    assert_eq!(machine.calls.last().map(String::as_str), Some("unlock"));
    assert!(!machine.calls.iter().any(|c| c.starts_with("set_password")));
    assert_eq!(machine.state, before);
}

#[test]
fn a_re_run_makes_a_deleted_account_again_under_a_new_sid() {
    let mut machine = FakeMachine::new("setup-rerun-deleted");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.accounts.remove("ClauseoSb02");
    let req = request(&machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.made.len(), 1);
    assert_eq!(report.made[0].name, "ClauseoSb02");
    assert_eq!(report.made[0].sid, sid(1005));
    assert_eq!(report.dropped, [sid(1002)], "no ledger entry names it");
    assert_eq!(report.kept_dead, []);
    let record = machine.state.record.clone().unwrap();
    assert_eq!(record.pool, [sid(1001), sid(1003), sid(1004), sid(1005)]);
    assert_eq!(report.reset.len(), 3);
}

#[test]
fn a_dead_sid_the_ledger_names_stays_recorded_and_is_made_again_once() {
    let mut machine = FakeMachine::new("setup-rerun-named");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let matters = two_matters(&machine);
    let req = request(&machine, 4, matters.clone());
    set_up(&mut machine, &mut acl, &req).unwrap();
    // The fence's ledger entries name every pool user; one account is then deleted by hand.
    machine.state.accounts.remove("ClauseoSb06");
    let req = request(&machine, 0, matters.clone());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.kept_dead, [sid(1006)]);
    assert_eq!(report.dropped, []);
    assert_eq!(report.made.len(), 1);
    assert_eq!(
        (report.made[0].name.as_str(), &report.made[0].sid),
        ("ClauseoSb06", &sid(1009))
    );
    let record = machine.state.record.clone().unwrap();
    assert!(record.pool.contains(&sid(1006)), "its entries stay trusted");
    assert!(record.pool.contains(&sid(1009)));
    assert_eq!(report.pool.len(), 8);
    assert!(
        !machine
            .state
            .marker
            .clone()
            .unwrap()
            .pool
            .contains(&sid(1006))
    );
    for matter in &matters {
        let root = canonical(&matter.folders.root);
        assert!(acl.has(&root, &sid(1009), Rights::DenyReadWrite));
    }

    // Run again: the dead SID was made again already, so nothing more is made.
    let from = machine.calls.len();
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(report.made.is_empty(), "{:?}", report.made);
    assert_eq!(report.kept_dead, [sid(1006)]);
    assert!(
        !machine
            .calls_since(from)
            .iter()
            .any(|c| c.starts_with("create_user"))
    );
}

#[test]
fn an_extend_at_the_cap_refuses_before_any_change() {
    let mut machine = FakeMachine::new("setup-cap");
    let mut acl = FakeAcl::default();
    let req = request(&machine, 32, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.pool.len(), 32);
    assert_eq!(report.pool[31].name, "ClauseoSb32");
    let before = machine.state.clone();
    let req = request(&machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::Pool, true));
    assert!(failure.cause.contains("32"), "{}", failure.cause);
    assert_eq!(machine.state, before);
}
