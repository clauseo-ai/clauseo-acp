//! Installs held by the updater (`Updater::install_held`: the desktop's repair runs the installer
//! itself): nothing begins an install, `auto`'s cached install at start included, and the update
//! stays `ready` for later.

use super::*;

/// `auto` with last run's download: the check offers the same version, which would install at
/// once; held, it is only `ready`, and installs once the hold is gone and the lawyer asks.
#[test]
fn update_auto_at_start_does_not_install_the_cached_download_while_installs_are_held() {
    let start = Start {
        cached: Some(CachedDownload {
            version: NEXT.to_owned(),
            path: PathBuf::from(format!("/data/updates/{NEXT}/Clauseo-setup.exe")),
        }),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (mut m, fx) = Machine::start(start, t0());
    assert!(checks(&fx), "{fx:?}");
    m.hold_install(true);
    let (mut m, fx) = go(&m, Input::CheckDone(Ok(Some(offer(NEXT)))), t0());
    assert!(!installs(&fx), "{fx:?}");
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, ready);
    m.hold_install(false);
    let (_, fx) = go(&m, Input::InstallRequested(InstallWhen::Now), t0());
    assert!(installs(&fx), "{fx:?}");
}

/// A `now` install that stopped the turns, or a schedule whose turns ended, begins nothing while
/// held: `ready`, and the gate on new turns lifted.
#[test]
fn update_a_held_install_goes_back_to_ready_with_the_gate_lifted() {
    let mut m = stopping_turns();
    m.hold_install(true);
    let (m, fx) = go(&m, Input::TurnsCancelled, t0());
    assert!(!installs(&fx), "{fx:?}");
    assert!(fx.contains(&Effect::GateNewTurns(false)), "{fx:?}");
    assert!(matches!(m.view().state, UpdateState::Ready { .. }));

    let mut m = scheduled();
    m.hold_install(true);
    let (m, fx) = go(&m, Input::TurnsRunning(false), t0());
    assert!(!installs(&fx), "{fx:?}");
    assert!(matches!(m.view().state, UpdateState::Ready { .. }));
}
