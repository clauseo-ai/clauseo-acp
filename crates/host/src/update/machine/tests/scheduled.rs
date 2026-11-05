//! "Restart now" on a scheduled install: `InstallRequested(now)` upgrades the wait in place.

use super::*;

/// `scheduled`, with the offer its download came from.
fn scheduled_with_offer() -> (Machine, Offer) {
    let (m, o) = ready_with_turns();
    let m = go(&m, Input::InstallRequested(InstallWhen::AfterTurn), t0()).0;
    (m, o)
}

#[test]
fn update_now_while_scheduled_with_turns_running_gates_cancels_and_installs_on_turns_cancelled() {
    let (m, o) = scheduled_with_offer();
    let (m, fx) = go(&m, Input::InstallRequested(InstallWhen::Now), t0());
    let installing = UpdateState::Installing {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, installing);
    let expected = [
        Effect::GateNewTurns(true),
        Effect::CancelTurns,
        Effect::Publish,
    ];
    assert_eq!(fx, expected);
    assert!(m.gated());
    // The running bit dropping is not the cancelled turns' end.
    let (m, fx) = go(&m, Input::TurnsRunning(false), t0());
    assert!(!installs(&fx), "{fx:?}");
    let (_, fx) = go(&m, Input::TurnsCancelled, t0());
    let expected = [
        Effect::WriteMarker(marker(CURRENT, NEXT)),
        Effect::BeginInstall(downloaded(&o)),
    ];
    assert_eq!(fx, expected);
}

#[test]
fn update_cancel_while_an_upgraded_install_waits_is_ready_with_the_gate_released() {
    let (m, _) = scheduled_with_offer();
    let m = go(&m, Input::InstallRequested(InstallWhen::Now), t0()).0;
    let (m, fx) = go(&m, Input::CancelScheduled, t0());
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, ready);
    assert_eq!(fx, [Effect::GateNewTurns(false), Effect::Publish]);
    assert!(!m.gated());
}

#[test]
fn update_now_while_scheduled_with_no_turn_running_installs_at_once() {
    // `TurnsRunning(false)` itself installs a scheduled update, so reach a `scheduled` whose
    // turns have stopped without that flip stepped (the command and the flip racing in the actor).
    let (mut m, o) = scheduled_with_offer();
    m.turns_running = false;
    let (m, fx) = go(&m, Input::InstallRequested(InstallWhen::Now), t0());
    let expected = [
        Effect::WriteMarker(marker(CURRENT, NEXT)),
        Effect::BeginInstall(downloaded(&o)),
        Effect::Publish,
    ];
    assert_eq!(fx, expected);
    assert!(!m.gated());
}

#[test]
fn update_after_turn_while_scheduled_is_still_refused() {
    let (m, _) = scheduled_with_offer();
    let why = step(&m, Input::InstallRequested(InstallWhen::AfterTurn), t0()).err();
    assert_eq!(why, Some(Refusal::NotNow));
}
