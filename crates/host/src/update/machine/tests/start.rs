//! Start-up: the five managed cases, the marker, the cached download in `auto` and `notify`,
//! the check delays, and the daily `Managed` count.

use super::*;

fn cached(version: &str) -> CachedDownload {
    CachedDownload {
        version: version.to_owned(),
        path: PathBuf::from(format!("/data/updates/{version}/Clauseo-setup.exe")),
    }
}

/// The five ways a run is managed, each with a cached download that `auto` would install.
fn managed_starts() -> Vec<(ManagedBy, Start)> {
    let auto = mode(UpdateMode::Auto);
    let with_cache = |s: Start| Start {
        cached: Some(cached(NEXT)),
        ..s
    };
    vec![
        (ManagedBy::Package, begin(InstallKind::Package, auto)),
        (ManagedBy::PerMachine, begin(InstallKind::PerMachine, auto)),
        (
            ManagedBy::ReadOnlyLocation,
            begin(InstallKind::ReadOnlyLocation, auto),
        ),
        (
            ManagedBy::Policy,
            begin(InstallKind::CurrentUser, mode(UpdateMode::None)),
        ),
        // An unreadable policy source: `refuse` gives `none`, locked.
        (
            ManagedBy::Policy,
            begin(
                InstallKind::CurrentUser,
                ModeInForce {
                    mode: UpdateMode::None,
                    locked: true,
                },
            ),
        ),
    ]
    .into_iter()
    .map(|(by, s)| (by, with_cache(s)))
    .collect()
}

#[test]
fn update_managed_from_the_start_never_checks() {
    for (by, start) in managed_starts() {
        let (mut m, fx) = Machine::start(start, t0());
        assert_eq!(m.view().state, UpdateState::Managed { by });
        assert!(!checks(&fx), "{by:?}: {fx:?}");
        assert_eq!(
            step(&m, Input::CheckRequested, t0()).err(),
            Some(Refusal::NotNow)
        );
        // A week of timers.
        for _ in 0..7 {
            let due = m.timer().unwrap();
            let (next, fx) = go(&m, Input::TimerFired, due);
            assert!(!checks(&fx), "{by:?}: {fx:?}");
            m = next;
        }
    }
}

#[test]
fn update_managed_counts_once_a_day() {
    let (m, fx) = Machine::start(begin(InstallKind::PerMachine, mode(UpdateMode::Auto)), t0());
    assert!(fx.contains(&Effect::Count(UpdateResult::Managed)), "{fx:?}");
    let due = m.timer().unwrap();
    assert_eq!(due.as_millis() - t0().as_millis(), 24 * 60 * MIN);
    // Policy taking over within the day changes who manages it, not the day's count.
    let none = mode(UpdateMode::None);
    let (m, fx) = go(&m, Input::PolicyChanged(none), at(t0().as_millis() + MIN));
    assert!(
        !fx.contains(&Effect::Count(UpdateResult::Managed)),
        "{fx:?}"
    );
    let due = m.timer().unwrap();
    let (_, fx) = go(&m, Input::TimerFired, due);
    assert!(fx.contains(&Effect::Count(UpdateResult::Managed)), "{fx:?}");
}

#[test]
fn update_first_check_is_one_to_ten_minutes_after_start_then_every_fifty_to_seventy() {
    let (mut first, mut interval) = (Vec::new(), Vec::new());
    for seed in 0..5_000 {
        let start = Start {
            jitter: Jitter::new(seed),
            ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
        };
        let (m, fx) = Machine::start(start, t0());
        assert!(!checks(&fx), "no check at start: {fx:?}");
        let due = armed(&fx).unwrap();
        first.push(due.as_millis() - t0().as_millis());
        let (m, _) = go(&m, Input::TimerFired, due);
        let (_, fx) = go(&m, Input::CheckDone(Ok(None)), due);
        interval.push(armed(&fx).unwrap().as_millis() - due.as_millis());
    }
    let span = |v: &[i64]| (*v.iter().min().unwrap(), *v.iter().max().unwrap());
    let (lo, hi) = span(&first);
    assert!(lo >= MIN && hi <= 10 * MIN, "first: {lo}..{hi}");
    assert!(lo < 2 * MIN && hi > 9 * MIN, "first spreads: {lo}..{hi}");
    let (lo, hi) = span(&interval);
    assert!(lo >= 50 * MIN && hi <= 70 * MIN, "interval: {lo}..{hi}");
    assert!(
        lo < 52 * MIN && hi > 68 * MIN,
        "interval spreads: {lo}..{hi}"
    );
}

#[test]
fn update_the_same_seed_gives_the_same_delays() {
    let start = || {
        Machine::start(
            begin(InstallKind::CurrentUser, mode(UpdateMode::Auto)),
            t0(),
        )
    };
    assert_eq!(start().1, start().1);
}

#[test]
fn update_a_marker_from_the_running_version_is_a_failed_install() {
    let start = Start {
        marker: Some(marker(CURRENT, NEXT)),
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    let failed = UpdateState::Failed {
        version: Some(NEXT.to_owned()),
        at: t0(),
        stage: FailedStage::Install,
    };
    assert_eq!(m.view().state, failed);
    assert_eq!(m.view().updated_from, None);
    assert_eq!(
        fx[..2],
        [Effect::ClearMarker, Effect::Count(UpdateResult::Failed)]
    );
    // Not retried from the file at once: the next check is on the timer.
    assert!(!checks(&fx) && !installs(&fx), "{fx:?}");
    assert!(armed(&fx).is_some());
}

#[test]
fn update_a_marker_to_the_running_version_is_updated_from() {
    let start = Start {
        marker: Some(marker("0.3.9", CURRENT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    assert_eq!(m.view().state, UpdateState::Idle);
    assert_eq!(m.view().updated_from.as_deref(), Some("0.3.9"));
    assert_eq!(
        fx[..2],
        [Effect::ClearMarker, Effect::Count(UpdateResult::Installed)]
    );
}

#[test]
fn update_a_marker_of_other_versions_is_cleared_and_nothing_else() {
    let start = Start {
        marker: Some(marker("0.2.0", "0.3.0")),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    assert_eq!(m.view().state, UpdateState::Idle);
    assert_eq!(m.view().updated_from, None);
    assert_eq!(fx.first(), Some(&Effect::ClearMarker));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Count(_))), "{fx:?}");
}

#[test]
fn update_auto_installs_last_runs_download_at_start_after_a_check_offers_it_again() {
    let start = Start {
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    assert_eq!(fx, [Effect::StartCheck, Effect::Publish]);
    let o = offer(NEXT);
    let (m, fx) = go(&m, Input::CheckDone(Ok(Some(o.clone()))), t0());
    let from_file = Downloaded {
        version: NEXT.to_owned(),
        path: cached(NEXT).path,
        handle: o.handle,
    };
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::StartDownload(_))),
        "{fx:?}"
    );
    assert!(installs(&fx), "{fx:?}");
    assert_eq!(written(&m), [Effect::BeginInstall(from_file)]);
}

#[test]
fn update_auto_at_start_with_a_turn_running_schedules_the_cached_install() {
    let start = Start {
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let m = Machine::start(start, t0()).0;
    let (m, _) = go(&m, Input::TurnsRunning(true), t0());
    let (m, fx) = go(&m, Input::CheckDone(Ok(Some(offer(NEXT)))), t0());
    assert!(!installs(&fx), "{fx:?}");
    let scheduled = UpdateState::Scheduled {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, scheduled);
    let (_, fx) = go(&m, Input::TurnsRunning(false), t0());
    assert!(installs(&fx), "{fx:?}");
}

#[test]
fn update_auto_at_start_offline_waits_and_a_later_offer_is_ready_not_installed() {
    let start = Start {
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let m = Machine::start(start, t0()).0;
    let (m, fx) = go(&m, Input::CheckDone(Err(net())), t0());
    assert!(!installs(&fx), "{fx:?}");
    let due = m.timer().unwrap();
    let (m, _) = go(&m, Input::TimerFired, due);
    let (m, fx) = go(&m, Input::CheckDone(Ok(Some(offer(NEXT)))), due);
    assert!(!installs(&fx), "{fx:?}");
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::StartDownload(_))),
        "{fx:?}"
    );
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, ready);
}

#[test]
fn update_auto_at_start_with_a_newer_offer_downloads_it_and_does_not_install() {
    let start = Start {
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let m = Machine::start(start, t0()).0;
    let newer = offer("0.4.2");
    let (m, fx) = go(&m, Input::CheckDone(Ok(Some(newer.clone()))), t0());
    assert_eq!(fx, [Effect::StartDownload(newer.clone()), Effect::Publish]);
    let (_, fx) = go(&m, Input::DownloadDone(Ok(downloaded(&newer))), t0());
    assert!(!installs(&fx), "{fx:?}");
}

#[test]
fn update_notify_never_installs_at_start_and_ready_installs_only_on_install_requested() {
    let start = Start {
        cached: Some(cached(NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Notify))
    };
    let (m, fx) = Machine::start(start, t0());
    assert!(!checks(&fx), "{fx:?}");
    assert_eq!(m.view().state, UpdateState::Idle);
    let due = m.timer().unwrap();
    let (m, _) = go(&m, Input::TimerFired, due);
    let (m, fx) = go(&m, Input::CheckDone(Ok(Some(offer(NEXT)))), due);
    assert!(!installs(&fx), "{fx:?}");
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    assert_eq!(m.view().state, ready);
    for input in [Input::TurnsRunning(true), Input::TurnsRunning(false)] {
        let (_, fx) = go(&m, input, due);
        assert!(!installs(&fx), "{fx:?}");
    }
    let (_, fx) = go(&m, Input::InstallRequested(InstallWhen::AfterTurn), due);
    assert!(installs(&fx), "{fx:?}");
}

#[test]
fn update_a_cached_download_of_the_running_version_is_ignored() {
    let start = Start {
        cached: Some(cached(CURRENT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    assert!(!checks(&fx), "{fx:?}");
    assert_eq!(m.view().state, UpdateState::Idle);
}
