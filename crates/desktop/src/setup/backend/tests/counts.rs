//! The pump's usage counts (ruling 9, `count.rs`): one `agent_install` per install run that ends
//! and one `sign_in` per sign-in run that ends, none for a state the session did not run up to
//! (a receipt's `Ready` at start, a status check's `SignedIn`); and the quit's `shutdown` goes
//! to `Agents`.

use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::testing::failed;
use clauseo_agents::{AgentsEvent, FailureKind, InstallState, SigninCode, SigninState, Source};
use clauseo_counts::schema::{
    Agent, AgentInstall, Billing, Event, InstallResult, SignIn, SignInResult, Took, Tries,
};
use clauseo_protocol::{AgentId, CommandOutcome};
use serde_json::json;

use super::fakes::{command, rig};
use super::{KEY, MAIN, SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::setup::SetupBackend;
use crate::test_support::block_on;

fn install(agent: &AgentId, state: InstallState) -> AgentsEvent {
    AgentsEvent::Install {
        agent: agent.clone(),
        state,
    }
}

fn signin(agent: &AgentId, state: SigninState) -> AgentsEvent {
    AgentsEvent::Signin {
        agent: agent.clone(),
        state,
    }
}

fn downloading() -> InstallState {
    InstallState::Downloading {
        received: 1,
        total: 2,
    }
}

fn ready() -> InstallState {
    InstallState::Ready {
        version: "1.0.0".to_owned(),
    }
}

fn install_count(agent: Agent, result: InstallResult, tries: u8) -> Event {
    Event::AgentInstall(AgentInstall {
        agent,
        result,
        tries: Tries::new(tries).unwrap(),
        took: Took::Lt1s,
    })
}

fn signin_count(agent: Agent, result: SignInResult) -> Event {
    Event::SignIn(SignIn {
        agent,
        billing: Billing::Plan,
        result,
    })
}

#[test]
fn an_install_counts_once_per_end_and_never_at_start() {
    let rig = rig(true);
    lock(&rig.agents.tries).insert(codex(), 2);
    let events = vec![
        // A receipt's `Ready` at start, and a `Ready` after a pause: no run seen up to them.
        install(&claude(), ready()),
        install(&codex(), downloading()),
        install(&codex(), InstallState::Paused),
        install(&codex(), ready()),
        // Codex's run: offline for a while, then installed after two tries.
        install(&codex(), downloading()),
        install(
            &codex(),
            InstallState::Waiting {
                reason: FailureKind::Offline,
                log: clauseo_agents::InstallLog::new(&codex()),
            },
        ),
        install(&codex(), downloading()),
        install(&codex(), InstallState::Installing),
        install(&codex(), ready()),
        // The same `Ready` again (an `install` of a ready agent) counts nothing.
        install(&codex(), ready()),
        // Claude's run fails on a full disk; no tries recorded counts as one.
        install(&claude(), InstallState::Installing),
        install(&claude(), failed(&claude(), FailureKind::DiskFull)),
    ];
    block_on(rig.backend.pump(futures::stream::iter(events)));
    assert_eq!(
        *lock(&rig.counts.0),
        [
            install_count(Agent::Chatgpt, InstallResult::Done, 2),
            install_count(Agent::Claude, InstallResult::DiskFull, 1),
        ]
    );
}

#[test]
fn a_sign_in_counts_once_per_run_end() {
    let rig = rig(true);
    let at = UNIX_EPOCH + Duration::from_hours(500_000);
    let signed_in = || SigninState::SignedIn {
        email: Some("lawyer@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Own,
        at,
    };
    let failed = |code| SigninState::Failed { code, at };
    let events = vec![
        // The status check at start: signed in with no run.
        signin(&claude(), signed_in()),
        signin(
            &claude(),
            SigninState::Waiting {
                since: at,
                nudge: false,
            },
        ),
        signin(
            &claude(),
            SigninState::Waiting {
                since: at,
                nudge: true,
            },
        ),
        signin(&claude(), signed_in()),
        signin(&codex(), SigninState::Queued),
        signin(&codex(), failed(SigninCode::Cancelled)),
        // A failure with no run going counts nothing.
        signin(&codex(), failed(SigninCode::Failed)),
        signin(
            &codex(),
            SigninState::DeviceCode {
                url: "https://auth.openai.com/codex/device".to_owned(),
                code: "ABCD-EFGH".to_owned(),
                expires_at: at,
            },
        ),
        signin(&codex(), failed(SigninCode::LinkExpired)),
    ];
    block_on(rig.backend.pump(futures::stream::iter(events)));
    assert_eq!(
        *lock(&rig.counts.0),
        [
            signin_count(Agent::Claude, SignInResult::Done),
            signin_count(Agent::Chatgpt, SignInResult::Cancelled),
            signin_count(Agent::Chatgpt, SignInResult::Failed),
        ]
    );
}

#[test]
fn the_quit_shuts_agents_down() {
    let rig = rig(true);
    assert!(rig.backend.shutdown(Duration::from_secs(3)));
    assert!(lock(&rig.journal).contains(&"agents.shutdown".to_owned()));
    // An empty cell: `Agents` never started, so nothing is left running.
    let (cell, _start) = crate::setup::BackendCell::new();
    assert!(cell.shutdown(Duration::ZERO));
}

fn waiting() -> SigninState {
    SigninState::Waiting {
        since: UNIX_EPOCH,
        nudge: false,
    }
}

fn cancelled() -> SigninState {
    SigninState::Failed {
        code: SigninCode::Cancelled,
        at: UNIX_EPOCH,
    }
}

#[test]
fn a_key_or_an_endpoint_that_ends_a_sign_in_counts_none() {
    for (name, fields) in [
        ("set_agent_key", json!({ "key": KEY })),
        (
            "set_firm_endpoint",
            json!({ "url": "https://gateway.firm.example" }),
        ),
    ] {
        let rig = rig(true);
        let setup = rig.setup();
        let setting = command(name, &claude(), &fields);
        let answer = rig.pumped(async {
            rig.s18_sends(&claude(), waiting());
            let answer = setup.run(SETTINGS, setting).await;
            // S18 then ends the run the key or the endpoint took over.
            rig.s18_sends(&claude(), cancelled());
            answer
        });
        assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
        assert_eq!(*lock(&rig.counts.0), [], "{name}: not the lawyer's cancel");
    }
}

#[test]
fn a_lawyers_cancel_of_a_sign_in_counts_once() {
    let rig = rig(true);
    let setup = rig.setup();
    let cancel = command("cancel_sign_in", &claude(), &json!({}));
    let answer = rig.pumped(async {
        rig.s18_sends(&claude(), waiting());
        let answer = setup.run(MAIN, cancel).await;
        rig.s18_sends(&claude(), cancelled());
        answer
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(
        *lock(&rig.counts.0),
        [signin_count(Agent::Claude, SignInResult::Cancelled)]
    );
}
