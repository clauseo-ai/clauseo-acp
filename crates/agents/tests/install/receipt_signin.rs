//! A sign-in after the start sent `Ready` from a receipt: one pressed while the Node check
//! hangs runs at once, and fails cleanly when the receipt's Node cannot start; one pressed after
//! the check failed starts the repair and then runs at `Ready`. The fixture's first agent is
//! named `claude-acp` so it signs in as Claude; its run is the fixture adapter, which is enough
//! to show the sign-in started.

use clauseo_agents::{AgentEntry, AgentSizes, InstallState, Method, SigninState, Snapshot};
use clauseo_protocol::AgentId;

use crate::receipt_ready::{BOUND, Events, Gate, Mode, install_first, start_gated, until_within};
use crate::support::{A_PACKAGES, B_PACKAGES, BASE, Fixture, agent_b, node_entry, pins, platform};

pub(crate) fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

/// The install fixture with its first agent named `claude-acp`, and a home for its sign-in.
pub(crate) fn claude_fixture(label: &str) -> Fixture {
    let mut fx = Fixture::new(label);
    let (package_json, lock) = pins(BASE, &fx.tarballs);
    let platform = platform();
    let sized = |id: AgentId, root: &str, names: &[&str]| {
        let mut entry = AgentEntry::new(id, root, root, "dist/index.js");
        entry.install_hosts = vec!["registry.test".to_owned()];
        let download_bytes = names
            .iter()
            .map(|name| u64::try_from(fx.tarballs[name].len()).unwrap())
            .sum();
        entry.sizes = vec![AgentSizes {
            os: platform.os,
            arch: platform.arch,
            download_bytes,
            unpacked_bytes: 10_000,
        }];
        entry
    };
    let agents = vec![
        sized(claude(), "adapter-a", &A_PACKAGES),
        sized(agent_b(), "adapter-b", &B_PACKAGES),
    ];
    fx.config.snapshot =
        Snapshot::new(&package_json, &lock, BASE, node_entry(None), agents).unwrap();
    let home = fx.data.path().join("claude-home");
    crate::write::write(&home.join(".keep"), "").unwrap();
    fx.config.signin.homes.insert(claude(), home);
    fx
}

/// Sign-in states up to the first of Claude's for which `done` holds, under [`BOUND`] each.
pub(crate) fn signin_until(events: &Events, done: impl Fn(&SigninState) -> bool) -> SigninState {
    loop {
        let (agent, state) = events
            .signin
            .recv_timeout(BOUND)
            .expect("a sign-in state within the bound");
        if agent == claude() && done(&state) {
            return state;
        }
    }
}

#[test]
fn a_sign_in_during_a_hung_check_whose_node_cannot_start_fails_cleanly() {
    let fx = claude_fixture("receipt-signin-broken");
    install_first(&fx);
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();
    let first = until_within(&events, &claude(), |_| true);
    assert!(matches!(first, InstallState::Ready { .. }), "{first:?}");

    // The receipt's Node cannot start: the sign-in goes ahead at once, and its run fails.
    gate.fail_runs();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let ended = signin_until(&events, |state| matches!(state, SigninState::Failed { .. }));
    assert!(matches!(ended, SigninState::Failed { .. }), "{ended:?}");
    assert_eq!(gate.held(), 1, "the check is still held");

    gate.set(Mode::Pass);
    assert!(agents.shutdown(BOUND), "every thread ended");
}

#[test]
fn a_sign_in_after_a_failed_check_repairs_the_install_then_signs_in() {
    let fx = claude_fixture("receipt-signin-repair");
    let installed = install_first(&fx);
    let gate = Gate::new(Mode::Fail);
    let (agents, events) = start_gated(&fx, &gate);
    until_within(&events, &claude(), |state| {
        matches!(state, InstallState::Failed(_))
    });
    assert_eq!(agents.installed(&claude()), None);

    gate.set(Mode::Pass);
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    // The press itself starts the repair: no retry is called.
    let repaired = until_within(&events, &claude(), |state| {
        matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
    });
    assert!(
        matches!(repaired, InstallState::Ready { .. }),
        "{repaired:?}"
    );
    assert_eq!(agents.installed(&claude()), Some(installed));
    // The sign-in, queued until then, starts at Ready.
    let started = signin_until(&events, |state| !matches!(state, SigninState::Queued));
    assert!(!matches!(started, SigninState::Queued), "{started:?}");
    assert!(agents.shutdown(BOUND), "every thread ended");
}
