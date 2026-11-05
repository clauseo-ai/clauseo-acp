//! The launch refusals the host starts with and the keys' lift (decision record 0048, ruling
//! 4; the security review's M3): every known agent is refused from the host's start, a blocked
//! one for the policy and the rest until the keys load; the lift after the load clears only
//! the keys' refusal, never the policy's.

use std::collections::BTreeSet;

use clauseo_protocol::ErrorCode;

use super::fakes::{HostCall, rig};
use super::{claude, codex};
use crate::setup::start::{lift_keys_loading, refusals_at_start};
use crate::test_support::block_on;

#[test]
fn the_host_starts_refusing_a_blocked_agent_for_the_policy_and_the_rest_for_the_keys() {
    let blocked = BTreeSet::from([codex()]);
    assert_eq!(
        refusals_at_start([&claude(), &codex()], &blocked),
        [
            (claude(), ErrorCode::AgentKeysLoading),
            (codex(), ErrorCode::SigninPolicyUnreadable),
        ]
    );
    assert_eq!(
        refusals_at_start([&claude(), &codex()], &BTreeSet::new()),
        [
            (claude(), ErrorCode::AgentKeysLoading),
            (codex(), ErrorCode::AgentKeysLoading),
        ]
    );
}

#[test]
fn the_keys_lift_clears_only_the_agents_the_policy_does_not_block() {
    let rig = rig(true);
    let blocked = BTreeSet::from([codex()]);
    block_on(lift_keys_loading(
        &*rig.host,
        [&claude(), &codex()],
        &blocked,
    ));
    assert_eq!(
        rig.host.calls(),
        [HostCall::SetLaunchRefused(claude(), None)],
        "the policy's refusal of Codex stays"
    );
}

#[test]
fn the_backend_starts_knowing_the_host_refuses_the_blocked_agents() {
    let rig = rig(true);
    // The rig's start settings block nothing, so the host was told nothing to lift later.
    assert!(crate::bridge::lock(&rig.backend.wiring.refused).is_empty());
    let read = crate::setup::refresh::SettingsRead {
        blocked: BTreeSet::from([codex()]),
        ..Default::default()
    };
    let wiring = crate::setup::wiring::Wiring::new(
        std::sync::Arc::clone(&rig.backend.wiring.readers),
        rig.backend.wiring.reused.clone(),
        std::sync::Arc::clone(&rig.backend.wiring.plans),
        read,
    );
    assert_eq!(
        *crate::bridge::lock(&wiring.refused),
        BTreeSet::from([codex()])
    );
}
