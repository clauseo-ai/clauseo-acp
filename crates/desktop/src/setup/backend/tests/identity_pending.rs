//! R17 over a kept press (`pending.rs`): a sign-in pressed before its agent is installed runs
//! at the install's `Ready` through the identity check as a direct press does, so its
//! `SignedIn` records the new account and is never taken as an account change.

use clauseo_agents::{AccountHash, AgentsEvent, InstallState, Source};
use clauseo_protocol::CommandOutcome;
use serde_json::json;

use super::fakes::command;
use super::identity::{IdRig, signed_in};
use super::{MAIN, codex};
use crate::bridge::lock;
use crate::setup::identity::{Seen, on_state};
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

#[test]
fn a_sign_in_kept_until_ready_records_its_account_and_is_no_change() {
    let id = IdRig::new("identity-pending");
    let (rig, agent) = (&id.rig, codex());
    assert_eq!(id.signs_in(&agent, ONE), Seen::SignedIn);
    // The agent is not installed: the press is kept until the install's `Ready`.
    lock(&rig.agents.states).insert(
        agent.clone(),
        InstallState::Missing {
            download_bytes: 120_000_000,
        },
    );
    let answer = block_on(
        rig.setup()
            .run(MAIN, command("start_sign_in", &agent, &json!({}))),
    );
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert!(lock(&rig.backend.pending).contains_key(&agent), "kept");
    let ready = InstallState::Ready {
        version: "1.0.0".to_owned(),
    };
    lock(&rig.agents.states).insert(agent.clone(), ready.clone());
    let event = AgentsEvent::Install {
        agent: agent.clone(),
        state: ready,
    };
    block_on(rig.backend.pump(futures::stream::iter(vec![event])));
    assert!(
        rig.calls().contains(&("sign_in", agent.clone())),
        "the kept press signed in at Ready"
    );
    // Its `SignedIn`, with another account than the one recorded, is the sign-in's own.
    let state = signed_in(OTHER, Source::Own);
    let seen = block_on(on_state(&rig.backend, &agent, &state));
    assert_eq!(seen, Seen::SignedIn);
    assert_eq!(
        rig.backend.wiring.identity.recorded(&agent),
        AccountHash::of(OTHER)
    );
}
