//! A sign-in pressed before its agent is installed (`pending.rs`): it starts the install, is
//! kept as a pending press (`Queued`), and runs at the install's `Ready` through the direct
//! press's checks, taken then; `Waiting` keeps it, `Failed`, `Paused`, `Managed` and
//! `CancelSignIn` drop it, a second press replaces it; and no policy or managed path installs.

use clauseo_agents::testing::failed;
use clauseo_agents::{AgentsEvent, FailureKind, InstallLog, InstallState, Method};
use clauseo_protocol::{AgentId, CommandOutcome, SetupEvent, SignIn};
use serde_json::json;

use super::super::pending::Press;
use super::fakes::{Rig, command, rig, setup_events};
use super::{MAIN, claude, codex, missing};
use crate::bridge::lock;
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::setup::{KeyWrite, SetupBackend};
use crate::test_support::block_on;

fn install(agent: &AgentId, state: InstallState) -> AgentsEvent {
    AgentsEvent::Install {
        agent: agent.clone(),
        state,
    }
}

fn ready() -> InstallState {
    InstallState::Ready {
        version: "1.0.0".to_owned(),
    }
}

/// `agent` is not installed: `Missing`, as S18 says at start.
fn not_installed(rig: &Rig, agent: &AgentId) {
    lock(&rig.agents.states).insert(
        agent.clone(),
        InstallState::Missing {
            download_bytes: 120_000_000,
        },
    );
}

/// The window's `name` command for `agent`, from `main`; its answer.
fn press(rig: &Rig, name: &str, agent: &AgentId, fields: &serde_json::Value) {
    let answer = block_on(rig.setup().run(MAIN, command(name, agent, fields)));
    assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
}

/// The pump takes `states` for `agent`, in order; the install is `Ready` from the first
/// `Ready` on.
fn pump(rig: &Rig, agent: &AgentId, states: Vec<InstallState>) {
    if states
        .iter()
        .any(|state| matches!(state, InstallState::Ready { .. }))
    {
        lock(&rig.agents.states).insert(agent.clone(), ready());
    }
    let events: Vec<_> = states
        .into_iter()
        .map(|state| install(agent, state))
        .collect();
    block_on(rig.backend.pump(futures::stream::iter(events)));
}

fn pending(rig: &Rig, agent: &AgentId) -> Option<Press> {
    lock(&rig.backend.pending).get(agent).copied()
}

/// The sign-in states sent for `agent` on `events`.
fn sign_ins(events: Vec<SetupEvent>, agent: &AgentId) -> Vec<SignIn> {
    events
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent: of, state } if of == *agent => Some(state),
            _ => None,
        })
        .collect()
}

#[test]
fn a_sign_in_on_a_missing_agent_installs_it_then_signs_in_at_ready() {
    let cases = [
        (
            claude(),
            "start_sign_in",
            json!({}),
            Press::SignIn(Method::ClaudeAi),
            "sign_in",
        ),
        (
            claude(),
            "start_sign_in",
            json!({ "console": true }),
            Press::SignIn(Method::Console),
            "sign_in",
        ),
        (
            codex(),
            "start_sign_in",
            json!({}),
            Press::SignIn(Method::Browser),
            "sign_in",
        ),
        (
            codex(),
            "use_device_code",
            json!({}),
            Press::SignIn(Method::DeviceCode),
            "sign_in_device_code",
        ),
        (
            codex(),
            "use_different_account",
            json!({}),
            Press::DifferentAccount,
            "use_different_account",
        ),
    ];
    for (agent, name, fields, kept, call) in cases {
        // A confirm that declines: the press asks nothing.
        let rig = rig(false);
        not_installed(&rig, &agent);
        let mut feed = rig.events.follow();
        press(&rig, name, &agent, &fields);
        assert_eq!(
            rig.calls(),
            [("install", agent.clone())],
            "{name}: the install, no sign-in"
        );
        assert_eq!(pending(&rig, &agent), Some(kept), "{name}");
        assert_eq!(
            sign_ins(setup_events(&mut feed), &agent),
            [SignIn::Queued],
            "{name}: shown queued"
        );
        assert_eq!(*lock(&rig.confirm.asked), [], "{name}: no confirm");

        pump(&rig, &agent, vec![InstallState::Installing, ready()]);
        assert_eq!(
            rig.calls(),
            [("install", agent.clone()), (call, agent.clone())],
            "{name}: the sign-in at Ready, in place of the status check"
        );
        assert_eq!(pending(&rig, &agent), None, "{name}");
    }
}

/// A press on an installed agent signs in at once, as before: nothing is kept.
#[test]
fn a_sign_in_on_an_installed_agent_starts_at_once() {
    let rig = rig(true);
    press(&rig, "start_sign_in", &claude(), &json!({}));
    assert_eq!(rig.calls(), [("sign_in", claude())]);
    assert_eq!(pending(&rig, &claude()), None);
}

/// A key set while the install runs bars the sign-in at `Ready`: no sign-in, the usual
/// `signin.key_in_use` refusal (logged), the status checked instead, and `Queued` gone.
#[test]
fn a_bar_that_holds_at_ready_refuses_the_pending_sign_in() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    let mut feed = rig.events.follow();
    // The key reaches the composer without a key command's end of the sign-in (a key the
    // keychain finished loading, say): only the check at `Ready` stands in the way.
    rig.backend.compose.set_key(&claude(), true);
    pump(&rig, &claude(), vec![ready()]);
    assert_eq!(
        rig.calls(),
        [("install", claude()), ("check_status", claude())],
        "no sign-in"
    );
    assert_eq!(pending(&rig, &claude()), None);
    let shown = sign_ins(setup_events(&mut feed), &claude());
    assert!(
        !matches!(shown.last(), Some(SignIn::Queued) | None),
        "{shown:?}"
    );
}

/// A key set through its command while the press waits ends the press there, as it ends a
/// running sign-in: nothing runs at `Ready`.
#[test]
fn a_key_set_while_pending_drops_the_press() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    let key = command(
        "set_agent_key",
        &claude(),
        &json!({ "key": "sk-ant-api03-PENDING-TEST-KEY" }),
    );
    let answer = block_on(rig.setup().run(MAIN, key));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(pending(&rig, &claude()), None);
    pump(&rig, &claude(), vec![ready()]);
    assert!(
        !rig.calls().iter().any(|(name, _)| *name == "sign_in"),
        "{:?}",
        rig.calls()
    );
}

#[test]
fn waiting_keeps_the_press_and_failed_drops_it() {
    let rig = rig(true);
    not_installed(&rig, &codex());
    press(&rig, "start_sign_in", &codex(), &json!({}));
    let waiting = InstallState::Waiting {
        reason: FailureKind::Offline,
        log: InstallLog::new(&codex()),
    };
    pump(
        &rig,
        &codex(),
        vec![
            InstallState::Downloading {
                received: 1,
                total: 2,
            },
            waiting,
        ],
    );
    assert_eq!(
        pending(&rig, &codex()),
        Some(Press::SignIn(Method::Browser)),
        "kept while offline"
    );
    let mut feed = rig.events.follow();
    pump(&rig, &codex(), vec![failed(&codex(), FailureKind::Offline)]);
    assert_eq!(pending(&rig, &codex()), None, "dropped on Failed");
    let shown = sign_ins(setup_events(&mut feed), &codex());
    // A new follower gets the latest state first (`Queued`), then the change.
    assert!(
        matches!(shown.as_slice(), [SignIn::Queued, SignIn::SignedOut { .. }]),
        "{shown:?}"
    );
    // A `Ready` later (a retry) signs nothing in: a fresh press is needed.
    pump(&rig, &codex(), vec![ready()]);
    assert_eq!(
        rig.calls(),
        [("install", codex()), ("check_status", codex())]
    );
}

#[test]
fn paused_or_managed_drops_the_press() {
    for state in [InstallState::Paused, InstallState::Managed] {
        let rig = rig(true);
        not_installed(&rig, &claude());
        press(&rig, "start_sign_in", &claude(), &json!({}));
        pump(&rig, &claude(), vec![state.clone()]);
        assert_eq!(pending(&rig, &claude()), None, "{state:?}");
    }
}

/// `CancelSignIn` while the press waits drops it and answers `Done`; the install carries on
/// (no pause, no S18 cancel), and its `Ready` signs nothing in.
#[test]
fn cancel_while_pending_drops_the_press_and_the_install_carries_on() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    let mut feed = rig.events.follow();
    press(&rig, "cancel_sign_in", &claude(), &json!({}));
    assert_eq!(pending(&rig, &claude()), None);
    assert_eq!(rig.calls(), [("install", claude())], "the install alone");
    let shown = sign_ins(setup_events(&mut feed), &claude());
    // A new follower gets the latest state first (`Queued`), then the change.
    assert!(
        matches!(shown.as_slice(), [SignIn::Queued, SignIn::SignedOut { .. }]),
        "{shown:?}"
    );
    pump(&rig, &claude(), vec![ready()]);
    assert_eq!(
        rig.calls(),
        [("install", claude()), ("check_status", claude())]
    );
}

/// A second press while the first waits replaces it, past the sign-in join: the last wins.
#[test]
fn a_second_press_while_pending_replaces_the_first() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    press(
        &rig,
        "start_sign_in",
        &claude(),
        &json!({ "console": true }),
    );
    assert_eq!(
        pending(&rig, &claude()),
        Some(Press::SignIn(Method::Console))
    );
    assert_eq!(rig.calls(), [("install", claude()), ("install", claude())]);
}

/// How many sign-ins the fake `Agents` was asked to start.
fn sign_ins_started(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|(name, _)| *name == "sign_in")
        .count()
}

/// S18 says `Ready` before the pump has handled it (the pump writes the entry first): a press
/// then replaces the kept one rather than starting directly, so only the latest press signs in,
/// once, and no older press restarts it.
#[test]
fn a_press_between_ready_and_the_pump_replaces_the_kept_one() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    lock(&rig.agents.states).insert(claude(), ready());
    press(
        &rig,
        "start_sign_in",
        &claude(),
        &json!({ "console": true }),
    );
    assert_eq!(
        pending(&rig, &claude()),
        Some(Press::SignIn(Method::Console))
    );
    assert_eq!(sign_ins_started(&rig), 0, "nothing started directly");
    pump(&rig, &claude(), vec![ready()]);
    assert_eq!(
        sign_ins_started(&rig),
        1,
        "the latest press, once: {:?}",
        rig.calls()
    );
    assert_eq!(pending(&rig, &claude()), None);
}

/// A `StartSignIn` that went past the join while a press was kept, reaching the backend only
/// after `Ready` started that press: it joins the running sign-in, with no restart.
#[test]
fn a_press_that_skipped_the_join_joins_the_started_press() {
    let rig = rig(true);
    not_installed(&rig, &claude());
    press(&rig, "start_sign_in", &claude(), &json!({}));
    pump(&rig, &claude(), vec![ready()]);
    assert_eq!(sign_ins_started(&rig), 1);
    // The second press as `Setup` sends it once it saw the kept press: past the join.
    let late = command("start_sign_in", &claude(), &json!({}));
    let answer = block_on(rig.backend.run(late, KeyWrite::Replace, &rig.events));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(sign_ins_started(&rig), 1, "no restart: {:?}", rig.calls());
}

/// No start state, policy read, managed agent or firm policy starts an install: only a
/// lawyer's choice or sign-in press does.
#[test]
fn no_policy_or_managed_path_installs() {
    let rig = rig(true);
    block_on(rig.backend.pump(futures::stream::iter([
        missing(claude()),
        install(&codex(), InstallState::Managed),
        install(&codex(), InstallState::Managed),
    ])));
    let mut read = SettingsRead::default();
    read.network.upstream = Some("http://proxy.firm.example:3128".to_owned());
    block_on(apply_settings(&rig.backend, read));
    block_on(apply_settings(&rig.backend, SettingsRead::default()));
    for managed in [true, false] {
        rig.backend.set_firm_policy(&claude(), managed);
        rig.backend.set_firm_policy(&codex(), managed);
    }
    let starts: Vec<_> = rig
        .calls()
        .into_iter()
        .filter(|(name, _)| matches!(*name, "install" | "retry"))
        .collect();
    assert_eq!(starts, [], "no install and no retry");
}
