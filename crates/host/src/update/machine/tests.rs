//! The machine's transitions, one test per row of R23's table, and the install paths with turns
//! running. `start.rs` covers start-up (managed, the marker, the cached download, the delays);
//! `refusals.rs` every command in every state.

use std::path::PathBuf;

use clauseo_counts::schema::UpdateResult;
use clauseo_protocol::{
    Count, FailedStage, InstallWhen, ManagedBy, Timestamp, UpdateMode, UpdateState,
};

use super::{Effect, Input, Jitter, Machine, ModeInForce, Refusal, Start, step};
use crate::update::{
    CachedDownload, Downloaded, InstallKind, MarkerState, Offer, Opaque, Progress, UpdateMarker,
    UpdaterError,
};

mod refusals;
mod review;
mod scheduled;
mod start;

const MIN: i64 = 60_000;
const CURRENT: &str = "0.4.0";
const NEXT: &str = "0.4.1";

fn at(millis: i64) -> Timestamp {
    Timestamp::new(millis).unwrap()
}

/// Start-up's time.
fn t0() -> Timestamp {
    at(1_800_000_000_000)
}

fn mode(mode: UpdateMode) -> ModeInForce {
    ModeInForce {
        mode,
        locked: false,
    }
}

fn begin(kind: InstallKind, mode: ModeInForce) -> Start {
    Start {
        current: CURRENT.to_owned(),
        kind,
        mode,
        marker: None,
        cached: None,
        jitter: Jitter::new(7),
    }
}

fn offer(version: &str) -> Offer {
    Offer {
        version: version.to_owned(),
        handle: Opaque::new(version.to_owned()),
    }
}

fn downloaded(offer: &Offer) -> Downloaded {
    Downloaded {
        version: offer.version.clone(),
        path: PathBuf::from(format!("/data/updates/{}/Clauseo-setup.exe", offer.version)),
        handle: offer.handle.clone(),
    }
}

fn marker(from: &str, to: &str) -> UpdateMarker {
    UpdateMarker {
        from: from.to_owned(),
        to: to.to_owned(),
        state: MarkerState::Installing,
    }
}

/// A step that must be taken.
fn go(m: &Machine, input: Input, now: Timestamp) -> (Machine, Vec<Effect>) {
    step(m, input, now).unwrap()
}

fn armed(fx: &[Effect]) -> Option<Timestamp> {
    fx.iter().find_map(|e| {
        if let Effect::ArmTimer { at } = e {
            Some(*at)
        } else {
            None
        }
    })
}

fn checks(fx: &[Effect]) -> bool {
    fx.contains(&Effect::StartCheck)
}

fn installs(fx: &[Effect]) -> bool {
    fx.iter().any(|e| matches!(e, Effect::BeginInstall(_)))
}

// Each state, reached through the machine's own steps.

fn idle() -> Machine {
    Machine::start(
        begin(InstallKind::CurrentUser, mode(UpdateMode::Auto)),
        t0(),
    )
    .0
}

fn checking() -> Machine {
    go(&idle(), Input::CheckRequested, t0()).0
}

fn downloading() -> Machine {
    go(&checking(), Input::CheckDone(Ok(Some(offer(NEXT)))), t0()).0
}

fn ready() -> (Machine, Offer) {
    let o = offer(NEXT);
    let d = go(&checking(), Input::CheckDone(Ok(Some(o.clone()))), t0()).0;
    (go(&d, Input::DownloadDone(Ok(downloaded(&o))), t0()).0, o)
}

fn ready_with_turns() -> (Machine, Offer) {
    let (m, o) = ready();
    (go(&m, Input::TurnsRunning(true), t0()).0, o)
}

fn scheduled() -> Machine {
    let m = ready_with_turns().0;
    go(&m, Input::InstallRequested(InstallWhen::AfterTurn), t0()).0
}

/// `installing` under `now`, waiting for the cancelled turns to end.
fn stopping_turns() -> Machine {
    let m = ready_with_turns().0;
    go(&m, Input::InstallRequested(InstallWhen::Now), t0()).0
}

/// `installing` with `BeginInstall` gone out.
fn began() -> Machine {
    go(&ready().0, Input::InstallRequested(InstallWhen::Now), t0()).0
}

fn failed() -> Machine {
    go(&checking(), Input::CheckDone(Err(net())), t0()).0
}

fn managed() -> Machine {
    Machine::start(begin(InstallKind::Package, mode(UpdateMode::Auto)), t0()).0
}

fn net() -> UpdaterError {
    UpdaterError::Network("offline".to_owned())
}

#[test]
fn update_idle_or_failed_checks_when_its_timer_fires() {
    for m in [idle(), failed()] {
        let due = m.timer().unwrap();
        // Early, it arms again at the same time (tests/review.rs).
        let (early, fx) = go(&m, Input::TimerFired, at(due.as_millis() - 1));
        assert_eq!(fx, [Effect::ArmTimer { at: due }]);
        assert_eq!(early.view(), m.view());
        let (next, fx) = go(&m, Input::TimerFired, due);
        assert_eq!(next.view().state, UpdateState::Checking);
        assert_eq!(fx, [Effect::StartCheck, Effect::Publish]);
    }
}

#[test]
fn update_idle_or_failed_checks_on_check_requested_and_the_old_timer_goes_stale() {
    for m in [idle(), failed()] {
        let due = m.timer().unwrap();
        let (next, fx) = go(&m, Input::CheckRequested, t0());
        assert_eq!(next.view().state, UpdateState::Checking);
        assert_eq!(fx, [Effect::StartCheck, Effect::Publish]);
        let (_, fx) = go(&next, Input::TimerFired, due);
        assert!(fx.is_empty(), "{fx:?}");
    }
}

#[test]
fn update_checking_with_nothing_offered_is_idle_with_checked_at_and_the_next_check_armed() {
    let now = at(t0().as_millis() + 5 * MIN);
    let (m, fx) = go(&checking(), Input::CheckDone(Ok(None)), now);
    let view = m.view();
    assert_eq!(view.state, UpdateState::Idle);
    assert_eq!(view.checked_at, Some(now));
    let next = armed(&fx).unwrap().as_millis() - now.as_millis();
    assert!((50 * MIN..=70 * MIN).contains(&next), "{next}");
    assert_eq!(fx.last(), Some(&Effect::Publish));
}

#[test]
fn update_checking_with_an_offer_downloads_it_in_auto_and_notify() {
    for chosen in [UpdateMode::Auto, UpdateMode::Notify] {
        let m = Machine::start(begin(InstallKind::CurrentUser, mode(chosen)), t0()).0;
        let m = go(&m, Input::CheckRequested, t0()).0;
        let o = offer(NEXT);
        let (m, fx) = go(&m, Input::CheckDone(Ok(Some(o.clone()))), t0());
        let state = UpdateState::Downloading {
            version: NEXT.to_owned(),
            received: Count::ZERO,
            total: None,
        };
        assert_eq!(m.view().state, state, "{chosen:?}");
        assert_eq!(m.view().checked_at, Some(t0()));
        assert_eq!(fx, [Effect::StartDownload(o), Effect::Publish]);
    }
}

#[test]
fn update_downloading_shows_progress_then_is_ready_when_done() {
    let progress = Progress {
        received: 10,
        total: Some(40),
    };
    let (m, fx) = go(&downloading(), Input::Progress(progress), t0());
    let state = UpdateState::Downloading {
        version: NEXT.to_owned(),
        received: Count::clamped(10),
        total: Some(Count::clamped(40)),
    };
    assert_eq!((m.view().state, fx), (state, vec![Effect::Publish]));
    let (m, fx) = go(&m, Input::DownloadDone(Ok(downloaded(&offer(NEXT)))), t0());
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    assert_eq!((m.view().state, fx), (ready, vec![Effect::Publish]));
}

#[test]
fn update_ready_installs_now_at_once_with_no_turn_running() {
    let (m, o) = ready();
    let (m, fx) = go(&m, Input::InstallRequested(InstallWhen::Now), t0());
    let installing = UpdateState::Installing {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, installing);
    let expected = [
        Effect::WriteMarker(marker(CURRENT, NEXT)),
        Effect::BeginInstall(downloaded(&o)),
        Effect::Publish,
    ];
    assert_eq!(fx, expected);
    assert!(!m.gated());
}

#[test]
fn update_now_with_turns_running_gates_and_cancels_then_installs_only_on_turns_cancelled() {
    let (m, o) = ready_with_turns();
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
fn update_after_turn_installs_at_once_with_no_turn_running() {
    let (m, o) = ready();
    let (_, fx) = go(&m, Input::InstallRequested(InstallWhen::AfterTurn), t0());
    assert!(fx.contains(&Effect::BeginInstall(downloaded(&o))), "{fx:?}");
    assert!(!fx.contains(&Effect::Count(UpdateResult::AfterAnswer)));
}

#[test]
fn update_after_turn_is_scheduled_while_turns_run_and_installs_once_none_runs() {
    let (m, o) = ready_with_turns();
    let (m, fx) = go(&m, Input::InstallRequested(InstallWhen::AfterTurn), t0());
    let scheduled = UpdateState::Scheduled {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, scheduled);
    // No gate: new turns may start, and each pushes the install back.
    assert_eq!(fx, [Effect::Publish]);
    let (m, fx) = go(&m, Input::TurnsRunning(true), t0());
    assert!(fx.is_empty() && m.view().state == scheduled, "{fx:?}");
    let (_, fx) = go(&m, Input::TurnsCancelled, t0());
    assert!(!installs(&fx), "{fx:?}");
    let (m, fx) = go(&m, Input::TurnsRunning(false), t0());
    let expected = [
        Effect::WriteMarker(marker(CURRENT, NEXT)),
        Effect::Count(UpdateResult::AfterAnswer),
        Effect::BeginInstall(downloaded(&o)),
        Effect::Publish,
    ];
    assert_eq!(fx, expected);
    let installing = UpdateState::Installing {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, installing);
}

#[test]
fn update_cancel_takes_scheduled_back_to_ready_and_a_now_install_releases_the_gate() {
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    let (m, fx) = go(&scheduled(), Input::CancelScheduled, t0());
    assert_eq!((m.view().state, fx), (ready.clone(), vec![Effect::Publish]));
    let (_, fx) = go(&m, Input::TurnsRunning(false), t0());
    assert!(
        !installs(&fx),
        "a cancelled schedule never installs: {fx:?}"
    );
    let (m, fx) = go(&stopping_turns(), Input::CancelScheduled, t0());
    assert_eq!(m.view().state, ready);
    assert_eq!(fx, [Effect::GateNewTurns(false), Effect::Publish]);
    assert!(!m.gated());
    let (_, fx) = go(&m, Input::TurnsCancelled, t0());
    assert!(!installs(&fx), "{fx:?}");
}

#[test]
fn update_an_error_in_checking_or_downloading_is_failed_and_counted() {
    let now = at(t0().as_millis() + 3 * MIN);
    let (m, fx) = go(&checking(), Input::CheckDone(Err(net())), now);
    let check = UpdateState::Failed {
        version: None,
        at: now,
        stage: FailedStage::Check,
    };
    assert_eq!(m.view().state, check);
    assert_eq!(m.view().checked_at, None);
    assert_eq!(fx.first(), Some(&Effect::Count(UpdateResult::Failed)));
    assert!(armed(&fx).is_some(), "the next check: {fx:?}");
    let bad = UpdaterError::Signature("bad".to_owned());
    let (m, fx) = go(&downloading(), Input::DownloadDone(Err(bad)), now);
    let download = UpdateState::Failed {
        version: Some(NEXT.to_owned()),
        at: now,
        stage: FailedStage::Download,
    };
    assert_eq!(m.view().state, download);
    assert_eq!(fx.first(), Some(&Effect::Count(UpdateResult::Failed)));
}

#[test]
fn update_policy_none_makes_every_state_managed_but_an_install_that_began() {
    let locked_none = ModeInForce {
        mode: UpdateMode::None,
        locked: true,
    };
    let policy = UpdateState::Managed {
        by: ManagedBy::Policy,
    };
    let states = [
        ("idle", idle()),
        ("checking", checking()),
        ("downloading", downloading()),
        ("ready", ready().0),
        ("scheduled", scheduled()),
        ("stopping turns", stopping_turns()),
        ("failed", failed()),
        ("managed by package", managed()),
    ];
    for (name, m) in states {
        let (next, fx) = go(&m, Input::PolicyChanged(locked_none), t0());
        assert_eq!(next.view().state, policy, "{name}");
        assert_eq!(next.view().mode, UpdateMode::None, "{name}");
        let cancels = fx.contains(&Effect::CancelDownload);
        assert_eq!(cancels, name == "downloading", "{name}: {fx:?}");
        let ungates = fx.contains(&Effect::GateNewTurns(false));
        assert_eq!(ungates, name == "stopping turns", "{name}: {fx:?}");
        assert!(!next.gated(), "{name}");
        // A late result of the check or download changes nothing.
        let (late, fx) = go(&next, Input::CheckDone(Ok(Some(offer(NEXT)))), t0());
        assert!(fx.is_empty() && late.view().state == policy, "{name}");
        let (_, fx) = go(&next, Input::TurnsRunning(false), t0());
        assert!(!installs(&fx), "{name}: {fx:?}");
    }
    let (m, fx) = go(&began(), Input::PolicyChanged(locked_none), t0());
    let installing = UpdateState::Installing {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, installing);
    assert!(!fx.contains(&Effect::CancelTurns), "{fx:?}");
}

#[test]
fn update_leaving_managed_is_idle_with_the_first_check_one_to_ten_minutes_away() {
    let m = Machine::start(
        begin(InstallKind::CurrentUser, mode(UpdateMode::None)),
        t0(),
    )
    .0;
    let now = at(t0().as_millis() + 90 * MIN);
    let (m, fx) = go(&m, Input::PolicyChanged(mode(UpdateMode::Notify)), now);
    assert_eq!(m.view().state, UpdateState::Idle);
    assert_eq!(m.view().mode, UpdateMode::Notify);
    let delay = armed(&fx).unwrap().as_millis() - now.as_millis();
    assert!((MIN..=10 * MIN).contains(&delay), "{delay}");
    assert!(!checks(&fx), "{fx:?}");
}

#[test]
fn update_mode_set_between_auto_and_notify_changes_only_the_mode() {
    let (m, fx) = go(&ready().0, Input::ModeSet(UpdateMode::Notify), t0());
    assert_eq!(m.view().mode, UpdateMode::Notify);
    assert!(matches!(m.view().state, UpdateState::Ready { .. }));
    assert_eq!(fx, [Effect::Publish]);
}

#[test]
fn update_events_a_state_no_longer_concerns_change_nothing() {
    let (r, _) = ready();
    let stale = [
        (idle(), Input::CheckDone(Ok(Some(offer(NEXT))))),
        (idle(), Input::DownloadDone(Ok(downloaded(&offer(NEXT))))),
        (idle(), Input::TurnsCancelled),
        (idle(), Input::TurnsRunning(false)),
        (checking(), Input::DownloadDone(Err(net()))),
        (
            checking(),
            Input::Progress(Progress {
                received: 1,
                total: None,
            }),
        ),
        (r.clone(), Input::CheckDone(Err(net()))),
        (r.clone(), Input::TurnsCancelled),
        (r, Input::TimerFired),
        (scheduled(), Input::TurnsCancelled),
    ];
    for (m, input) in stale {
        let name = format!("{:?} + {input:?}", m.view().state);
        let (next, fx) = go(&m, input, t0());
        assert_eq!(next.view(), m.view(), "{name}");
        assert!(fx.is_empty(), "{name}: {fx:?}");
    }
}

#[test]
fn update_a_refused_command_reports_why() {
    assert_eq!(
        step(&idle(), Input::CancelScheduled, t0()).err(),
        Some(Refusal::NotNow)
    );
}
