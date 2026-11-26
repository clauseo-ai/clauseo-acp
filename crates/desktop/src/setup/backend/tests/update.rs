//! An agent's update (`backend/update.rs`): the lawyer added it before (its `agents.<id>` entry
//! is in the settings), and an app update's new pins left it with no install, so S18 installs it
//! again by itself (`AgentsConfig::reinstall`, tested in `agents`' `tests/install/reinstall.rs`).
//! The window never sees an "add"-style `Missing`: every state says `installed_before`. Its
//! conversations are held while it is not `Ready` (the queue's `update` line), through a
//! failed or cancelled download whose Try again starts it over, and go on at `Ready` once the
//! new entry is written. At `Ready` the status check answers how it stands, and no sign-in
//! starts; a press that meets that check waits for it instead of replacing it.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use clauseo_agents::testing::installed;
use clauseo_agents::{AgentsEvent, FailureKind, InstallLog, InstallState, SigninState, Source};
use clauseo_protocol::{AgentId, CommandOutcome, InstallProgress, SetupEvent, SignIn};
use serde_json::json;

use super::fakes::{HostCall, Rig, command, rig};
use super::switch_account::{asked, sign_ins, start};
use super::timing::until;
use super::{MAIN, SETTINGS, codex, setup_events};
use crate::bridge::lock;
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::test_support::block_on;

fn install(agent: &AgentId, state: InstallState) -> AgentsEvent {
    AgentsEvent::Install {
        agent: agent.clone(),
        state,
    }
}

fn downloading(received: u64) -> InstallState {
    InstallState::Downloading {
        received,
        total: 196_000_000,
    }
}

fn ready() -> InstallState {
    InstallState::Ready {
        version: "2.10.0".to_owned(),
    }
}

fn offline() -> InstallState {
    InstallState::Waiting {
        reason: FailureKind::Offline,
        log: InstallLog::new(&codex()),
    }
}

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some("lawyer@example.com".to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: UNIX_EPOCH,
    }
}

/// A rig whose settings say Codex was added here before, its new install's files known.
fn added_before() -> Rig {
    let rig = rig(true);
    let read = SettingsRead {
        installed: BTreeSet::from([codex()]),
        ..SettingsRead::default()
    };
    block_on(apply_settings(&rig.backend, read));
    let folder = PathBuf::from("/data/agents/codex-acp/install/new");
    lock(&rig.agents.installs).insert(
        codex(),
        installed(
            PathBuf::from("/data/node"),
            folder.join("index.js"),
            folder,
            "2.10.0",
        ),
    );
    rig
}

/// Codex's install states on `events`, each with its event's `installed_before`.
fn installs(events: &[SetupEvent]) -> Vec<(InstallProgress, bool)> {
    events
        .iter()
        .filter_map(|event| match event {
            SetupEvent::AgentInstallProgress {
                agent,
                state,
                installed_before,
            } if *agent == codex() => Some((state.clone(), *installed_before)),
            _ => None,
        })
        .collect()
}

fn codex_sign_ins(events: &[SetupEvent]) -> Vec<SignIn> {
    events
        .iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent, state } if *agent == codex() => Some(state.clone()),
            _ => None,
        })
        .collect()
}

/// The host's calls for Codex's hold, its entry and its restart, in order.
fn hold_calls(rig: &Rig) -> Vec<&'static str> {
    rig.host
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            HostCall::HoldForUpdate(agent) if agent == codex() => Some("hold_for_update"),
            HostCall::SetAgentLaunch(agent, _) if agent == codex() => Some("set_agent_launch"),
            HostCall::RestartAgents(agent) if agent == codex() => Some("restart_agents"),
            _ => None,
        })
        .collect()
}

/// Sends `states` for Codex to the pump [`Rig::pumped`] runs.
fn send(rig: &Rig, states: Vec<InstallState>) {
    let feed = lock(&rig.agents.feed);
    let feed = feed.as_ref().expect("only while pumped");
    for state in states {
        feed.unbounded_send(install(&codex(), state)).unwrap();
    }
}

#[test]
fn an_update_shows_no_add_holds_its_conversations_and_ends_signed_in_with_no_sign_in() {
    let rig = added_before();
    let mut feed = rig.events.follow();
    lock(&rig.agents.answers).insert("check_status", signed_in());
    rig.pumped(async {
        send(
            &rig,
            vec![
                downloading(0),
                downloading(98_000_000),
                InstallState::Installing,
            ],
        );
        lock(&rig.agents.states).insert(codex(), ready());
        send(&rig, vec![ready()]);
        // The fake answers the check on the feed, so the work ends only once it is asked.
        until("the check asked", || asked(&rig, "check_status") == 1).await;
    });
    let events = setup_events(&mut feed);
    let shown = installs(&events);
    assert_eq!(shown.len(), 4, "{shown:?}");
    assert!(
        shown
            .iter()
            .all(|(state, before)| *before && !matches!(state, InstallProgress::Missing { .. })),
        "every state says installed_before, and none is Missing: {shown:?}"
    );
    assert!(matches!(shown[3].0, InstallProgress::Ready { .. }));
    let sign_ins_shown = codex_sign_ins(&events);
    assert!(
        !sign_ins_shown
            .iter()
            .any(|state| matches!(state, SignIn::SignedOut { .. })),
        "not shown signed out while it updates: {sign_ins_shown:?}"
    );
    assert!(
        matches!(sign_ins_shown.last(), Some(SignIn::SignedIn { .. })),
        "the status check reports the login: {sign_ins_shown:?}"
    );
    assert_eq!(asked(&rig, "check_status"), 1, "{:?}", rig.calls());
    assert_eq!(sign_ins(&rig), 0, "no sign-in started: {:?}", rig.calls());
    assert_eq!(
        asked(&rig, "install"),
        0,
        "S18 installs it, not the backend"
    );
    let calls = hold_calls(&rig);
    assert_eq!(
        calls[..3],
        ["hold_for_update", "set_agent_launch", "restart_agents"],
        "held once, lifted after the new entry: {calls:?}"
    );
    assert!(lock(&rig.backend.update_holds).is_empty());
}

#[test]
fn a_download_that_cannot_run_keeps_the_hold_offers_try_again_and_goes_on_at_ready() {
    let rig = added_before();
    let mut feed = rig.events.follow();
    rig.pumped(async {
        send(&rig, vec![downloading(0), offline()]);
    });
    let shown = installs(&setup_events(&mut feed));
    assert!(
        matches!(shown.last(), Some((InstallProgress::Waiting { .. }, true))),
        "offline, still added: {shown:?}"
    );
    assert_eq!(hold_calls(&rig), ["hold_for_update"], "held, not lifted");

    // Try again in the conversation or in Plans: the download starts over.
    lock(&rig.agents.states).insert(codex(), offline());
    let retry = command("retry_agent_install", &codex(), &json!({}));
    assert_eq!(
        block_on(rig.setup().run(MAIN, retry)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(asked(&rig, "retry"), 1, "{:?}", rig.calls());

    // Cancelled: `Missing`, still added, still held; its Try again installs it.
    rig.pumped(async {
        send(
            &rig,
            vec![
                downloading(0),
                InstallState::Missing {
                    download_bytes: 196_000_000,
                },
            ],
        );
    });
    let shown = installs(&setup_events(&mut feed));
    assert!(
        matches!(
            shown.last(),
            Some((
                InstallProgress::Missing {
                    installed_before: true,
                    ..
                },
                true
            ))
        ),
        "{shown:?}"
    );
    lock(&rig.agents.states).insert(
        codex(),
        InstallState::Missing {
            download_bytes: 196_000_000,
        },
    );
    let retry = command("retry_agent_install", &codex(), &json!({}));
    assert_eq!(
        block_on(rig.setup().run(MAIN, retry)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(asked(&rig, "install"), 1, "{:?}", rig.calls());
    assert_eq!(
        hold_calls(&rig),
        ["hold_for_update"],
        "held once, through it all"
    );

    // Ready: the new entry, then the hold lifted, once.
    rig.pumped(async {
        send(&rig, vec![downloading(0)]);
        lock(&rig.agents.states).insert(codex(), ready());
        send(&rig, vec![ready()]);
    });
    assert_eq!(
        hold_calls(&rig),
        ["hold_for_update", "set_agent_launch", "restart_agents"]
    );
}

#[test]
fn an_agent_never_added_is_neither_held_nor_said_installed_before() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    rig.pumped(async {
        send(
            &rig,
            vec![
                InstallState::Missing {
                    download_bytes: 196_000_000,
                },
                downloading(0),
            ],
        );
    });
    let shown = installs(&setup_events(&mut feed));
    assert!(shown.iter().all(|(_, before)| !before), "{shown:?}");
    assert_eq!(hold_calls(&rig), Vec::<&str>::new());
}

/// The window's "Sign in" at `Ready` meets the status check the backend asked: it waits for the
/// check's answer and is answered signed in, with no sign-in started and no check cancelled.
#[test]
fn a_sign_in_press_racing_the_ready_check_waits_for_it_and_ends_signed_in() {
    let rig = added_before();
    let answer = rig.pumped(async {
        lock(&rig.agents.states).insert(codex(), ready());
        send(&rig, vec![ready()]);
        until("the check asked", || {
            lock(&rig.backend.ready_checks).contains_key(&codex())
        })
        .await;
        let setup = rig.setup();
        let press = setup.run(MAIN, start(&codex()));
        let check_answers = async {
            until("the press waits", || {
                lock(&rig.backend.ready_waiters).contains_key(&codex())
            })
            .await;
            rig.s18_sends(&codex(), signed_in());
        };
        futures::future::join(press, check_answers).await.0
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(sign_ins(&rig), 0, "no sign-in: {:?}", rig.calls());
    assert_eq!(asked(&rig, "cancel_sign_in"), 0, "the check not cancelled");
    assert_eq!(asked(&rig, "check_status"), 1);
    assert!(lock(&rig.backend.ready_waiters).is_empty());
}

/// A key change while the update downloads (`clear_agent_key`) sets the hold again rather than
/// lift it: messages keep waiting with the update line, never failing to launch the install
/// that is gone, and `Ready` lifts it, once.
#[test]
fn a_key_change_during_the_download_keeps_the_hold_until_ready() {
    let rig = added_before();
    let cleared = rig.pumped(async {
        send(&rig, vec![downloading(0)]);
        until("held", || {
            lock(&rig.backend.update_holds).contains(&codex())
        })
        .await;
        let clear = command("clear_agent_key", &codex(), &json!({}));
        let cleared = rig.setup().run(SETTINGS, clear).await;
        lock(&rig.agents.states).insert(codex(), ready());
        send(&rig, vec![ready()]);
        until("lifted", || {
            !lock(&rig.backend.update_holds).contains(&codex())
        })
        .await;
        cleared
    });
    assert_eq!(cleared, Ok(CommandOutcome::Done));
    assert_eq!(
        hold_calls(&rig),
        [
            "hold_for_update",
            "hold_for_update",
            "set_agent_launch",
            "restart_agents"
        ],
        "held again, not lifted, until Ready: {:?}",
        rig.host.calls()
    );
}

/// A brand-new install's "Sign in" at `Ready` does not wait for the status check: its browser
/// opens at once. Only an update's press waits (above).
#[test]
fn a_new_installs_sign_in_press_does_not_wait_for_the_ready_check() {
    let rig = rig(true);
    let answer = rig.pumped(async {
        lock(&rig.agents.states).insert(codex(), ready());
        send(&rig, vec![ready()]);
        until("the check asked", || {
            lock(&rig.backend.ready_checks).contains_key(&codex())
        })
        .await;
        rig.setup().run(MAIN, start(&codex())).await
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert!(
        lock(&rig.backend.ready_waiters).is_empty(),
        "the press did not wait for the check"
    );
    assert_eq!(sign_ins(&rig), 1, "a sign-in started: {:?}", rig.calls());
}
