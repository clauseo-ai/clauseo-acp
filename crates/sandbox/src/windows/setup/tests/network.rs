//! Set-up's network step (S15 PR7): every live pool user covered, an extend covering the new
//! one, a WFP failure failing and rolling back the set-up, and a firewall that Group Policy
//! ignores, or that fails in any other way, leaving the set-up done on WFP's filters alone; WFP
//! failing (the Base Filtering Engine down included) fails the set-up. WSL's deny (step 8b)
//! comes after it, as admin, and its failure is only reported.

use super::*;
use crate::windows::wfp::spec::FIREWALL_UNDER_POLICY;
use crate::windows::wfp::{FirewallOutcome, FirewallTrouble, WfpError};

/// The SIDs `rids` names, as the fake makes them.
fn sids(rids: &[u32]) -> Vec<Sid> {
    rids.iter().map(|&rid| sid(rid)).collect()
}

#[test]
fn a_set_up_covers_every_pool_user_before_its_marker() {
    let mut machine = FakeMachine::new("network-fresh");
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);
    let pool = sids(&[1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008]);
    assert_eq!(report.network.covered, pool);
    assert_eq!(report.network.filters, 14, "6 blocks and 8 permits");
    assert_eq!(report.network.firewall, FirewallOutcome::Applied);
    let networks = &machine.around.networks;
    assert_eq!(networks.len(), 1);
    assert_eq!(networks[0].pool(), pool);
    assert_eq!(networks[0].owner(), &owner());
    let at = |line: &str| {
        machine
            .calls
            .iter()
            .position(|c| c.starts_with(line))
            .unwrap()
    };
    assert!(at("owner_of DeviceSync") < at("network"));
    assert!(at("network") < at("protect_for_owner"));
    assert!(at("network") < at("write_marker"));
}

#[test]
fn wsls_deny_comes_first_and_last_as_admin_and_its_failure_is_only_reported() {
    let mut machine = FakeMachine::new("wsl-deny");
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert_eq!(report.wsl, Ok(crate::windows::wsl::WslApplied::default()));
    // First once the group and the record exist, before the network; last after the marker.
    let wsl: Vec<usize> = (0..machine.calls.len())
        .filter(|at| machine.calls[*at] == "wsl_deny 1000")
        .collect();
    let at = |line: &str| machine.calls.iter().position(|c| c == line).unwrap();
    assert_eq!(wsl.len(), 2, "{:?}", machine.calls);
    assert!(wsl[0] > at("write_record 1001") && wsl[0] < at("program_data"));
    assert!(wsl[1] > at("write_marker"));
    // The first pass failing is made up by the last, whose outcome is the report's.
    machine.fail("wsl_deny 1000");
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(report.wsl.is_ok(), "{:?}", report.wsl);
    // Both failing leave the set-up done: the launch check refuses while a deny is missing.
    machine.fail("wsl_deny 1000");
    machine.fail("wsl_deny 1000");
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(report.wsl.is_err(), "{:?}", report.wsl);
    assert!(machine.state.marker.is_some());
}

#[test]
fn an_extend_covers_the_new_user_and_a_rerun_the_same_pool() {
    let mut machine = FakeMachine::new("network-extend");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let req = request(&mut machine, 1, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    let grown: Vec<Sid> = report.pool.iter().map(|user| user.sid.clone()).collect();
    assert!(grown.len() > 8, "the extend made users");
    assert_eq!(report.network.covered, grown);
    let req = request(&mut machine, 0, Vec::new());
    let again = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(again.network.covered, grown);
    let networks = &machine.around.networks;
    assert_eq!(networks.len(), 3);
    assert_eq!(
        networks[1], networks[2],
        "a re-run asks for the same objects"
    );
}

#[test]
fn a_wfp_failure_fails_the_set_up_and_rolls_it_back() {
    let mut machine = FakeMachine::new("network-fails");
    let mut acl = FakeAcl::default();
    machine.fail("network 1001,1002,1003,1004,1005,1006,1007,1008");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Network);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back, "{failure}");
    assert!(
        machine.state.record.is_none(),
        "the users this run made are gone"
    );
    assert!(
        machine.state.marker.is_none(),
        "no setup.json says the set-up is done"
    );
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c.starts_with("protect_for_owner"))
    );
}

#[test]
fn a_firewall_group_policy_ignores_leaves_the_set_up_done_on_wfp_alone() {
    let mut machine = FakeMachine::new("network-gp");
    let mut acl = FakeAcl::default();
    let why = format!("{FIREWALL_UNDER_POLICY} (LocalPolicyModifyState S_OK, state 2)");
    machine.around.firewall = Some(FirewallOutcome::Ineffective(why.clone()));
    let report = set_up_fresh(&mut machine, &mut acl);
    assert_eq!(report.network.firewall, FirewallOutcome::Ineffective(why));
    assert_eq!(report.network.filters, 14, "every filter is still built");
    assert!(machine.state.marker.is_some());
    assert!(FIREWALL_UNDER_POLICY.contains("AllowLocalPolicyMerge"));
}

#[test]
fn a_firewall_that_fails_with_wfp_ok_passes_with_a_note() {
    for trouble in [
        FirewallTrouble::ServiceStopped,
        FirewallTrouble::TurnedOff,
        FirewallTrouble::from_call("Rules::Add", 0x8000_4005),
    ] {
        let mut machine = FakeMachine::new("network-firewall-fails");
        let mut acl = FakeAcl::default();
        machine.around.firewall = Some(FirewallOutcome::Ineffective(trouble.words()));
        let report = set_up_fresh(&mut machine, &mut acl);
        assert_eq!(
            report.network.firewall,
            FirewallOutcome::Ineffective(trouble.words()),
            "{trouble:?}"
        );
        assert_eq!(report.network.filters, 14, "{trouble:?}");
        assert!(
            machine.state.marker.is_some(),
            "{trouble:?}: set-up is done"
        );
    }
}

#[test]
fn wfp_failing_fails_the_set_up_naming_the_base_filtering_engine() {
    let mut machine = FakeMachine::new("network-bfe-down");
    let mut acl = FakeAcl::default();
    let bfe = WfpError {
        call: "FwpmEngineOpen0",
        code: 1753,
    };
    machine.fails.push((
        "network 1001,1002,1003,1004,1005,1006,1007,1008".to_owned(),
        MachineError::Other(bfe.cause()),
    ));
    let req = request(&mut machine, 0, Vec::new());
    let outcome = set_up(&mut machine, &mut acl, &req);
    let failure = outcome.clone().unwrap_err();
    assert_eq!(failure.step, Step::Network);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back);
    assert!(machine.state.marker.is_none());
    // What the app reads back is a failed set-up (SetUpFailed), its cause naming BFE.
    let told = result_file::SetupResult::of(&outcome);
    let code = u32::from(SetupExit::Failed.as_code());
    match result_file::outcome(code, Some(&Ok(Some(told)))) {
        Err(crate::SetupError::Failed(why)) => {
            assert!(why.contains("Base Filtering Engine service (BFE)"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}
