//! A reused login and the sign-ins over it (the final security review's M1): a reused login
//! refuses `StartSignIn` and `UseDeviceCode` from either window with `command.invalid`, the code
//! its `SignOut` gets, and starts no S18 run; and a leave given up whose stop job a sign-in
//! replaced, that then failed, asks S18's status check, then runs the steps after the stop when
//! the reuse ended, or keeps everything reused when it did not.

use std::time::UNIX_EPOCH;

use clauseo_agents::{SigninCode, SigninState, Source};
use clauseo_protocol::{CommandOutcome, ErrorCode, SignIn};
use futures::future::join;
use serde_json::json;

use super::fakes::{HostCall, Rig, command};
use super::leave::{reused_codex, signed_in, stop_using_command};
use super::switching::shown;
use super::timing::{until, yield_now};
use super::{MAIN, SETTINGS, codex};
use crate::bridge::lock;
use crate::setup::leave::Wait;
use crate::test_support::block_on;

#[test]
fn a_reused_login_refuses_start_sign_in_and_use_device_code_with_sign_out_s_code() {
    let rig = reused_codex();
    let sign_out = block_on(
        rig.setup()
            .run(SETTINGS, command("sign_out", &codex(), &json!({}))),
    );
    let code = sign_out.unwrap_err().code();
    assert_eq!(code, ErrorCode::CommandInvalid);
    for label in [MAIN, SETTINGS] {
        for name in ["start_sign_in", "use_device_code"] {
            let start = command(name, &codex(), &json!({}));
            let refused = block_on(rig.setup().run(label, start)).unwrap_err();
            assert_eq!(refused.code(), code, "{label} {name}");
        }
    }
    assert_eq!(rig.calls(), [], "no S18 run started");
    assert!(rig.host.calls().is_empty(), "nothing told the host");
    assert!(rig.backend.wiring.reused.get(&codex()), "still reused");
}

/// A leave of Codex given up, whose stop job a sign-in replaced (`Waiting`), which then failed;
/// S18's status check then answers `checked`. Runs until `done` holds.
fn a_replaced_stop_fails(checked: SigninState, done: impl Fn(&Rig) -> bool) -> Rig {
    let rig = reused_codex();
    lock(&rig.agents.answers).insert("check_status", checked);
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        assert_eq!(answer, Ok(CommandOutcome::Done));
        rig.journal();
        let waiting = SigninState::Waiting {
            since: UNIX_EPOCH,
            nudge: false,
        };
        rig.s18_sends(&codex(), waiting);
        let failed = SigninState::Failed {
            code: SigninCode::Cancelled,
            at: UNIX_EPOCH,
        };
        rig.s18_sends(&codex(), failed);
        until("the check's answer handled", || done(&rig)).await;
        for _ in 0..50 {
            yield_now().await;
        }
    });
    rig
}

#[test]
fn a_replaced_stop_that_fails_while_still_reused_keeps_everything_reused_and_lifts_the_hold() {
    let restarted = |rig: &Rig| rig.host.calls().contains(&HostCall::RestartAgents(codex()));
    let rig = a_replaced_stop_fails(signed_in(Source::Reused), restarted);
    assert_eq!(
        rig.journal(),
        [
            "agents.check_status",
            "host.restart_agents",
            "host.login_changed",
            "readers.release",
        ],
        "S18 says reused: no steps after the stop, the hold lifted"
    );
    assert!(rig.backend.wiring.reused.get(&codex()), "still reused");
    assert!(lock(&rig.readers.entries).is_empty(), "no entry rewritten");
    assert!(matches!(
        shown(&rig, &codex()),
        Some(SignIn::SignedIn {
            source: clauseo_protocol::SignInSource::Reused,
            ..
        })
    ));
    assert!(
        rig.backend
            .wiring
            .leaves
            .begin(&codex(), &rig.backend.host)
            .is_ok(),
        "slot free"
    );
}

#[test]
fn a_replaced_stop_that_fails_after_the_reuse_ended_runs_the_steps_after_the_stop() {
    let told = |rig: &Rig| rig.host.calls().contains(&HostCall::LoginChanged(codex()));
    let rig = a_replaced_stop_fails(SigninState::SignedOut { found: None }, told);
    assert_eq!(
        rig.journal(),
        [
            "agents.check_status",
            "host.set_reused_login",
            "host.remove_login_links",
            "host.set_agent_launch",
            "readers.set_entry",
            "readers.release",
            "host.login_changed",
        ],
        "S18 says signed out: the steps after the stop, then the rule (nothing usable: held)"
    );
    assert!(!rig.backend.wiring.reused.get(&codex()), "the reuse ended");
    assert!(
        rig.host
            .calls()
            .contains(&HostCall::SetReusedLogin(codex(), false))
    );
    assert_eq!(lock(&rig.readers.entries).len(), 1, "the entry is Own");
    assert!(
        !rig.host.calls().contains(&HostCall::RestartAgents(codex())),
        "held for the next login"
    );
    assert_eq!(
        shown(&rig, &codex()),
        Some(SignIn::SignedOut { found: None })
    );
    assert!(
        rig.backend
            .wiring
            .leaves
            .begin(&codex(), &rig.backend.host)
            .is_ok(),
        "slot free"
    );
}
