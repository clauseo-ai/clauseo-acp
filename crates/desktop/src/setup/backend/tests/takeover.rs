//! An automatic leave and a new login on the recording fakes (reviews A1, H1′, A3): a running
//! sign-in skips the tries without being cancelled and, once it fails, `Switching` shows again
//! and the next try runs; a sign-in arriving during a try ends the leave and launches on it; a
//! "Use it" that finishes while a try waits on `stop_agents` is not stopped by it; a leave that
//! panics lifts its hold and ends `Switching`, and the next leave runs.

use clauseo_agents::{Found, SigninCode, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{SignIn, SignInSource};
use futures::channel::oneshot;
use futures::future::join;

use super::codex;
use super::fakes::HostCall;
use super::leave::reused_codex;
use super::switching::{SPLIT, count, own_login, retry, shown, switching};
use super::timing::until;
use crate::bridge::lock;
use crate::setup::leave::{Wait, auto_leave};

/// The names of the calls S18 was asked.
fn asked(rig: &super::fakes::Rig) -> Vec<&'static str> {
    rig.calls().into_iter().map(|(name, _)| name).collect()
}

#[test]
fn a_running_sign_in_skips_the_tries_and_once_it_fails_switching_shows_and_they_resume() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    let agent = codex();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    let drive = async {
        script.asked(retry(5)).await;
        // The lawyer started a sign-in meanwhile: S18 has reserved its job, with no state yet.
        lock(&rig.agents.signing_in).insert(codex());
        script.fire(retry(5)).await;
        script.asked(retry(30)).await;
        let asked = asked(&rig);
        assert!(
            !asked.contains(&"stop_using_login") && !asked.contains(&"cancel_sign_in"),
            "S18 was asked nothing: {asked:?}"
        );
        assert_eq!(
            count(&rig, &HostCall::StopAgents(codex())),
            1,
            "the try skipped"
        );
        assert_eq!(
            count(&rig, &HostCall::RestartAgents(codex())),
            0,
            "still held"
        );
        // The sign-in fails: `Switching` shows again, and the next try runs.
        lock(&rig.agents.signing_in).remove(&codex());
        let failed = SigninState::Failed {
            code: SigninCode::Failed,
            at: std::time::UNIX_EPOCH,
        };
        rig.s18_sends(&codex(), failed);
        until("Switching again", || {
            shown(&rig, &codex()) == Some(switching(false))
        })
        .await;
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        lock(&rig.agents.answers).insert("stop_using_login", SPLIT);
        script.fire(retry(30)).await;
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert_eq!(count(&rig, &HostCall::StopAgents(codex())), 2);
    assert_eq!(
        count(&rig, &HostCall::RestartAgents(codex())),
        1,
        "the try that finished"
    );
    assert!(!rig.backend.wiring.reused.get(&codex()));
}

#[test]
fn a_sign_in_arriving_mid_try_ends_switching_lifts_the_hold_and_launches_on_it() {
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let agent = codex();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    let drive = async {
        script.fire(Wait::SignedOut).await;
        script.asked(retry(5)).await;
        let shows = shown(&rig, &codex());
        assert!(matches!(shows, Some(SignIn::Switching { .. })), "{shows:?}");
        script.fire(retry(5)).await;
        script.asked(Wait::SignedOut).await;
        rig.s18_sends(&codex(), own_login());
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert!(matches!(
        shown(&rig, &codex()),
        Some(SignIn::SignedIn {
            source: SignInSource::Own,
            ..
        })
    ));
    let calls = rig.host.calls();
    let own = calls
        .iter()
        .position(|call| *call == HostCall::SetReusedLogin(codex(), false));
    let restart = calls
        .iter()
        .position(|call| *call == HostCall::RestartAgents(codex()));
    assert!(
        own.is_some() && own < restart,
        "told Own, then launched: {calls:?}"
    );
    assert_eq!(count(&rig, &HostCall::RestartAgents(codex())), 1);
    let stops = asked(&rig)
        .into_iter()
        .filter(|name| *name == "stop_using_login");
    assert_eq!(stops.count(), 2, "no try after the sign-in");
    assert!(!rig.backend.wiring.reused.get(&codex()));
}

#[test]
fn a_use_it_finishing_while_a_try_waits_on_stop_agents_is_not_stopped_by_it() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    let agent = codex();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    let used = SigninState::SignedIn {
        email: Some("other@firm.example".to_owned()),
        plan: None,
        source: Source::Reused,
        at: std::time::UNIX_EPOCH,
    };
    let drive = async {
        script.asked(retry(5)).await;
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        lock(&rig.agents.answers).insert("stop_using_login", SPLIT);
        let (open, gate) = oneshot::channel();
        *lock(&rig.host.stop_gate) = Some(gate);
        script.fire(retry(5)).await;
        let waits = || count(&rig, &HostCall::StopAgents(codex())) == 2;
        until("the try waits on stop_agents", waits).await;
        rig.s18_sends(&codex(), used);
        let handled = || count(&rig, &HostCall::RestartAgents(codex())) == 1;
        until("the pump handled the login", handled).await;
        open.send(()).unwrap();
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert!(
        !asked(&rig).contains(&"stop_using_login"),
        "{:?}",
        asked(&rig)
    );
    assert!(
        rig.backend.wiring.reused.get(&codex()),
        "the lawyer's login, reused"
    );
    assert!(matches!(
        shown(&rig, &codex()),
        Some(SignIn::SignedIn {
            source: SignInSource::Reused,
            ..
        })
    ));
    assert_eq!(
        count(&rig, &HostCall::RestartAgents(codex())),
        2,
        "the pump's, and the leave's own: the pump's may have come before its stop_agents"
    );
}

#[test]
fn a_panicking_automatic_leave_lifts_its_hold_ends_switching_and_the_next_leave_runs() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    let (told, restarted) = std::sync::mpsc::channel();
    *lock(&rig.host.restarted) = Some(told);
    let leaves = &rig.backend.wiring.leaves;
    rig.pumped_with_leaves(async {
        leaves.submit(&codex(), Some(Found::CodexLinkSplit));
        script.asked(retry(5)).await;
        assert_eq!(shown(&rig, &codex()), Some(switching(false)));
        *lock(&rig.host.stop_panics) = true;
        script.fire(retry(5)).await;
        let panicked = || count(&rig, &HostCall::StopAgents(codex())) == 2;
        until("the try that panics", panicked).await;
        restarted
            .recv_timeout(crate::test_support::STEP_WAIT)
            .expect("the hold lifted from its own thread");
        let ended = || !matches!(shown(&rig, &codex()), Some(SignIn::Switching { .. }));
        until("Switching ended", ended).await;
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        lock(&rig.agents.answers).insert("stop_using_login", SPLIT);
        leaves.submit(&codex(), Some(Found::CodexLinkSplit));
        let done = || count(&rig, &HostCall::LoginChanged(codex())) == 1;
        until("the next leave", done).await;
    });
    assert_eq!(count(&rig, &HostCall::StopAgents(codex())), 3);
    assert!(
        !rig.backend.wiring.reused.get(&codex()),
        "the next leave left"
    );
}
