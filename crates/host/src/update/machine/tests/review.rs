//! The host thread's review of #434: a mode change after start-up never lets the cached download
//! install by itself, an early timer re-arms rather than stopping every later check, and failures
//! count always when the lawyer asked or the install failed, once a day otherwise.

use super::*;

fn cached_auto_start() -> Machine {
    let start = Start {
        cached: Some(CachedDownload {
            version: NEXT.to_owned(),
            path: PathBuf::from("/data/updates/0.4.1/Clauseo-setup.exe"),
        }),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    Machine::start(start, t0()).0
}

#[test]
fn update_notify_chosen_while_the_start_check_runs_never_installs_the_cached_download() {
    let notify_locked = ModeInForce {
        mode: UpdateMode::Notify,
        locked: true,
    };
    for input in [
        Input::ModeSet(UpdateMode::Notify),
        Input::PolicyChanged(notify_locked),
    ] {
        let name = format!("{input:?}");
        let (m, _) = go(&cached_auto_start(), input, t0());
        let (m, fx) = go(&m, Input::CheckDone(Ok(Some(offer(NEXT)))), t0());
        assert!(!installs(&fx), "{name}: {fx:?}");
        // The file still saves the download.
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::StartDownload(_))),
            "{name}"
        );
        let ready = UpdateState::Ready {
            version: NEXT.to_owned(),
        };
        assert_eq!(m.view().state, ready, "{name}");
    }
}

#[test]
fn update_an_early_timer_re_arms_at_its_due_time() {
    // The actor's timer is one-shot: a wall clock stepped back makes it fire before `due`.
    for (name, m) in [("idle", idle()), ("managed", managed())] {
        let due = m.timer().unwrap();
        let (next, fx) = go(&m, Input::TimerFired, at(due.as_millis() - 1));
        assert_eq!(fx, [Effect::ArmTimer { at: due }], "{name}");
        assert_eq!(next.timer(), Some(due), "{name}");
        // And it still does its work when it fires on time.
        let (_, fx) = go(&next, Input::TimerFired, due);
        let works = if name == "idle" {
            checks(&fx)
        } else {
            fx.contains(&Effect::Count(UpdateResult::Managed))
        };
        assert!(works, "{name}: {fx:?}");
    }
}

fn failures(fx: &[Effect]) -> usize {
    fx.iter()
        .filter(|e| **e == Effect::Count(UpdateResult::Failed))
        .count()
}

#[test]
fn update_background_failures_count_once_a_day() {
    let day = 24 * 60 * MIN;
    let mut m = idle();
    let (mut counted, mut tries) = (0, 0);
    let mut first: Option<i64> = None;
    // A day offline: each timer's check fails, and once the download the check started.
    loop {
        let due = m.timer().unwrap();
        if first.is_some_and(|f| due.as_millis() >= f + day) {
            break;
        }
        let (next, _) = go(&m, Input::TimerFired, due);
        let (next, fx) = if tries == 5 {
            let (next, _) = go(&next, Input::CheckDone(Ok(Some(offer(NEXT)))), due);
            go(&next, Input::DownloadDone(Err(net())), due)
        } else {
            go(&next, Input::CheckDone(Err(net())), due)
        };
        counted += failures(&fx);
        first.get_or_insert(due.as_millis());
        tries += 1;
        m = next;
    }
    assert!(tries >= 20, "{tries}");
    assert_eq!(counted, 1);
    // The next day counts again.
    let due = m.timer().unwrap();
    let (next, _) = go(&m, Input::TimerFired, due);
    let (_, fx) = go(&next, Input::CheckDone(Err(net())), due);
    assert_eq!(failures(&fx), 1, "{fx:?}");
}

#[test]
fn update_failures_the_lawyer_asked_for_always_count() {
    let mut m = idle();
    for _ in 0..3 {
        let (next, _) = go(&m, Input::CheckRequested, t0());
        let (next, fx) = go(&next, Input::CheckDone(Err(net())), t0());
        assert_eq!(failures(&fx), 1, "check: {fx:?}");
        m = next;
    }
    // A download the lawyer's check started counts too.
    let (next, _) = go(&m, Input::CheckRequested, t0());
    let (next, _) = go(&next, Input::CheckDone(Ok(Some(offer(NEXT)))), t0());
    let (_, fx) = go(&next, Input::DownloadDone(Err(net())), t0());
    assert_eq!(failures(&fx), 1, "download: {fx:?}");
}

#[test]
fn update_a_failed_install_counts_even_after_a_background_failure_that_day() {
    let start = Start {
        marker: Some(marker(CURRENT, NEXT)),
        ..begin(InstallKind::CurrentUser, mode(UpdateMode::Auto))
    };
    let (m, fx) = Machine::start(start, t0());
    assert_eq!(failures(&fx), 1, "{fx:?}");
    // The marker's count is not the day's background one: the first offline check counts.
    let due = m.timer().unwrap();
    let (next, _) = go(&m, Input::TimerFired, due);
    let (_, fx) = go(&next, Input::CheckDone(Err(net())), due);
    assert_eq!(failures(&fx), 1, "{fx:?}");
}
