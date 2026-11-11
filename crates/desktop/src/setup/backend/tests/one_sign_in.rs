//! Sign-ins run one at a time (`pending.rs`): a sign-in press for one agent while the other's
//! sign-in is going is kept, shown `Queued`, starts no install of an installed agent and no
//! status check at its `Ready`, and starts by itself, with no new command, once the other
//! sign-in ends, however it ends; nothing the other agent does drops it.

use std::time::UNIX_EPOCH;

use clauseo_agents::{
    AgentsEvent, InstallState, LoginFrom, Method, SigninCode, SigninState, Source,
};
use clauseo_protocol::{AgentId, CommandOutcome, SignIn};
use serde_json::json;

use super::super::pending::Press;
use super::fakes::{Rig, command, rig, setup_events};
use super::pending::{not_installed, pending, pump, ready, sign_ins};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::setup::{KeyWrite, SetupBackend};
use crate::test_support::block_on;

/// The window's `name` command for `agent`, from `main`, answered `Done`.
fn press(rig: &Rig, name: &str, agent: &AgentId) {
    let answer = block_on(rig.setup().run(MAIN, command(name, agent, &json!({}))));
    assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
}

/// `agent`'s sign-in ends with `end` (`None`: a job that ended with no state), as S18 ends it:
/// the job freed first, then its last state to the pump.
fn sign_in_ends(rig: &Rig, agent: &AgentId, end: Option<SigninState>) {
    lock(&rig.agents.signing_in).remove(agent);
    let event = match end {
        Some(state) => {
            lock(&rig.agents.signin).insert(agent.clone(), state.clone());
            AgentsEvent::Signin {
                agent: agent.clone(),
                state,
            }
        }
        None => AgentsEvent::SigninUnchanged {
            agent: agent.clone(),
        },
    };
    block_on(rig.backend.pump(futures::stream::iter([event])));
}

/// Every way a sign-in ends: signed in, cancelled (Codex's status check after a cancel says
/// signed out), failed, a login found, and no state at all.
fn ends() -> Vec<(&'static str, Option<SigninState>)> {
    let failed = |code| SigninState::Failed {
        code,
        at: UNIX_EPOCH,
    };
    vec![
        (
            "signed in",
            Some(SigninState::SignedIn {
                email: None,
                plan: None,
                source: Source::Own,
                at: UNIX_EPOCH,
            }),
        ),
        ("cancelled", Some(failed(SigninCode::Cancelled))),
        (
            "cancelled, then checked",
            Some(SigninState::SignedOut { found: None }),
        ),
        ("failed", Some(failed(SigninCode::Failed))),
        (
            "a login found",
            Some(SigninState::LoginFound {
                from: LoginFrom::Codex,
                email: None,
            }),
        ),
        ("no state", None),
    ]
}

/// How many times the fake `Agents` was asked `name` for `agent`.
fn asked(rig: &Rig, name: &str, agent: &AgentId) -> usize {
    rig.calls()
        .iter()
        .filter(|(called, of)| *called == name && of == agent)
        .count()
}

#[test]
fn a_second_plan_s_sign_in_waits_for_the_first_then_starts_by_itself_at_its_end() {
    for (how, end) in ends() {
        let rig = rig(true);
        not_installed(&rig, &codex());
        press(&rig, "start_sign_in", &claude());
        assert!(lock(&rig.agents.signing_in).contains(&claude()), "{how}");
        press(&rig, "start_sign_in", &codex());
        assert_eq!(asked(&rig, "install", &codex()), 1, "{how}: installing");
        let mut feed = rig.events.follow();
        pump(&rig, &codex(), vec![InstallState::Installing, ready()]);
        assert_eq!(asked(&rig, "sign_in", &codex()), 0, "{how}: not started");
        assert_eq!(asked(&rig, "check_status", &codex()), 0, "{how}: no check");
        assert_eq!(
            pending(&rig, &codex()),
            Some(Press::SignIn(Method::Browser)),
            "{how}: kept"
        );
        let shown = sign_ins(setup_events(&mut feed), &codex());
        assert_eq!(shown.last(), Some(&SignIn::Queued), "{how}: {shown:?}");

        sign_in_ends(&rig, &claude(), end);
        assert_eq!(
            asked(&rig, "sign_in", &codex()),
            1,
            "{how}: started with no new command: {:?}",
            rig.calls()
        );
        assert_eq!(pending(&rig, &codex()), None, "{how}");
    }
}

/// A press for an installed agent while the other's sign-in is going: kept, with no install
/// asked for, and started when that one ends.
#[test]
fn a_press_for_an_installed_agent_waits_for_the_other_sign_in() {
    for (name, call, kept) in [
        ("start_sign_in", "sign_in", Press::SignIn(Method::Browser)),
        (
            "use_device_code",
            "sign_in_device_code",
            Press::SignIn(Method::DeviceCode),
        ),
        (
            "use_different_account",
            "use_different_account",
            Press::DifferentAccount,
        ),
    ] {
        let rig = rig(true);
        press(&rig, "start_sign_in", &claude());
        let mut feed = rig.events.follow();
        press(&rig, name, &codex());
        assert_eq!(rig.calls(), [("sign_in", claude())], "{name}: kept only");
        assert_eq!(pending(&rig, &codex()), Some(kept), "{name}");
        let shown = sign_ins(setup_events(&mut feed), &codex());
        assert_eq!(shown.last(), Some(&SignIn::Queued), "{name}: {shown:?}");

        sign_in_ends(
            &rig,
            &claude(),
            Some(SigninState::SignedOut { found: None }),
        );
        assert_eq!(
            rig.calls(),
            [("sign_in", claude()), (call, codex())],
            "{name}: started at the other's end"
        );
        assert_eq!(pending(&rig, &codex()), None, "{name}");
    }
}

/// The other agent's install `Ready` and `Paused` leave the kept press alone, and its sign-in's
/// end runs it rather than dropping it.
#[test]
fn the_other_agent_s_install_and_end_never_drop_the_kept_press() {
    let rig = rig(true);
    press(&rig, "start_sign_in", &claude());
    press(&rig, "start_sign_in", &codex());
    pump(&rig, &claude(), vec![ready(), InstallState::Paused]);
    assert_eq!(
        pending(&rig, &codex()),
        Some(Press::SignIn(Method::Browser)),
        "kept through the other's Ready and Paused"
    );
    assert_eq!(asked(&rig, "sign_in", &codex()), 0);
    sign_in_ends(&rig, &claude(), None);
    assert_eq!(asked(&rig, "sign_in", &codex()), 1, "{:?}", rig.calls());
}

/// The other sign-in ends while the agent still installs: the press stays kept, and runs at
/// its own `Ready`.
#[test]
fn an_end_before_the_install_is_ready_keeps_the_press_for_ready() {
    let rig = rig(true);
    not_installed(&rig, &codex());
    press(&rig, "start_sign_in", &claude());
    press(&rig, "start_sign_in", &codex());
    pump(&rig, &codex(), vec![InstallState::Installing]);
    sign_in_ends(
        &rig,
        &claude(),
        Some(SigninState::SignedOut { found: None }),
    );
    assert_eq!(asked(&rig, "sign_in", &codex()), 0, "not installed yet");
    assert_eq!(
        pending(&rig, &codex()),
        Some(Press::SignIn(Method::Browser))
    );
    pump(&rig, &codex(), vec![ready()]);
    assert_eq!(asked(&rig, "sign_in", &codex()), 1, "{:?}", rig.calls());
    assert_eq!(pending(&rig, &codex()), None);
}

/// A status check, a sign-out or a search of the other agent is no sign-in: a press goes on.
#[test]
fn only_a_sign_in_makes_a_press_wait() {
    let rig = rig(true);
    rig.backend.recheck_sign_in(&claude());
    for name in ["sign_out", "find_login"] {
        let other = command(name, &claude(), &json!({}));
        let answer = block_on(rig.backend.run(other, KeyWrite::Replace, &rig.events));
        assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
    }
    press(&rig, "start_sign_in", &codex());
    assert_eq!(asked(&rig, "sign_in", &codex()), 1, "{:?}", rig.calls());
    assert_eq!(pending(&rig, &codex()), None);
}

/// A press that waited at its install's `Ready` (no status check ran then) and is cancelled
/// afterwards owes its agent that check, run at the cancel, so a login already there shows
/// without a restart; one that starts once the other sign-in ends owes none.
#[test]
fn a_press_that_waited_at_ready_gets_its_status_check_when_it_never_runs() {
    let rig = rig(true);
    not_installed(&rig, &codex());
    press(&rig, "start_sign_in", &claude());
    press(&rig, "start_sign_in", &codex());
    pump(&rig, &codex(), vec![InstallState::Installing, ready()]);
    assert_eq!(
        asked(&rig, "check_status", &codex()),
        0,
        "none while it waits"
    );
    press(&rig, "cancel_sign_in", &codex());
    assert_eq!(pending(&rig, &codex()), None);
    assert_eq!(
        asked(&rig, "check_status", &codex()),
        1,
        "{:?}",
        rig.calls()
    );
    assert_eq!(asked(&rig, "sign_in", &codex()), 0);

    let rig = super::fakes::rig(true);
    not_installed(&rig, &codex());
    press(&rig, "start_sign_in", &claude());
    press(&rig, "start_sign_in", &codex());
    pump(&rig, &codex(), vec![InstallState::Installing, ready()]);
    sign_in_ends(
        &rig,
        &claude(),
        Some(SigninState::SignedOut { found: None }),
    );
    assert_eq!(asked(&rig, "sign_in", &codex()), 1);
    assert_eq!(
        asked(&rig, "check_status", &codex()),
        0,
        "{:?}",
        rig.calls()
    );
}
