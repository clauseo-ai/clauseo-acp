//! The sign-in flag (`agents/<id>/.chosen`, S18's `mark_chosen` and `clear_chosen`): written on
//! every Sign in press, kept or direct, and never on `InstallAgent` or `RetryAgentInstall`;
//! cleared on `CancelSignIn`, a sign-out and a successful sign-in.

use std::time::UNIX_EPOCH;

use clauseo_agents::{AgentsEvent, InstallState, SigninState, Source};
use clauseo_protocol::{AgentId, CommandOutcome};
use serde_json::json;

use super::fakes::{Rig, command, rig};
use super::{MAIN, SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::test_support::block_on;

fn press(rig: &Rig, name: &str, agent: &AgentId, fields: &serde_json::Value) {
    // Sign-out is the settings window's alone.
    let window = if name == "sign_out" { SETTINGS } else { MAIN };
    let answer = block_on(rig.setup().run(window, command(name, agent, fields)));
    assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
}

fn chosen(rig: &Rig) -> Vec<(&'static str, AgentId)> {
    lock(&rig.agents.chosen).clone()
}

fn not_installed(rig: &Rig, agent: &AgentId) {
    lock(&rig.agents.states).insert(
        agent.clone(),
        InstallState::Missing {
            download_bytes: Some(1_000),
        },
    );
}

#[test]
fn every_sign_in_press_writes_the_flag_kept_or_direct() {
    let cases = [
        (claude(), "start_sign_in", json!({})),
        (codex(), "start_sign_in", json!({})),
        (codex(), "use_device_code", json!({})),
        (codex(), "use_different_account", json!({})),
    ];
    for (agent, name, fields) in cases {
        for installed in [false, true] {
            let rig = rig(true);
            if !installed {
                not_installed(&rig, &agent);
            }
            press(&rig, name, &agent, &fields);
            assert_eq!(
                chosen(&rig),
                [("mark_chosen", agent.clone())],
                "{name}, installed {installed}"
            );
        }
    }
}

#[test]
fn install_agent_and_retry_write_no_flag() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "install_agent", &claude(), &json!({}));
    lock(&rig.agents.states).insert(claude(), InstallState::Paused);
    press(
        &rig,
        "retry_agent_install",
        &claude(),
        &json!({ "use_system_proxy": false }),
    );
    assert_eq!(
        rig.calls(),
        [("install", claude()), ("retry", claude())],
        "both ran"
    );
    assert_eq!(chosen(&rig), [], "no flag");
}

#[test]
fn cancel_clears_the_flag_for_a_kept_press_and_a_running_sign_in() {
    // Kept (the agent is not installed, so the press waits for the install), then direct.
    for installed in [false, true] {
        let rig = rig(true);
        if !installed {
            not_installed(&rig, &claude());
        }
        press(&rig, "start_sign_in", &claude(), &json!({}));
        press(&rig, "cancel_sign_in", &claude(), &json!({}));
        assert_eq!(
            chosen(&rig),
            [("mark_chosen", claude()), ("clear_chosen", claude())],
            "installed {installed}"
        );
    }
}

#[test]
fn sign_out_clears_the_flag() {
    let rig = rig(true);
    press(&rig, "sign_out", &claude(), &json!({}));
    assert_eq!(chosen(&rig), [("clear_chosen", claude())]);
}

#[test]
fn a_successful_sign_in_clears_the_flag_and_other_states_do_not() {
    let rig = rig(true);
    block_on(rig.backend.pump(futures::stream::iter([
        AgentsEvent::Signin {
            agent: claude(),
            state: SigninState::Queued,
        },
        AgentsEvent::Signin {
            agent: claude(),
            state: SigninState::SignedIn {
                email: None,
                plan: None,
                source: Source::Own,
                at: UNIX_EPOCH,
            },
        },
    ])));
    assert_eq!(chosen(&rig), [("clear_chosen", claude())]);
}
