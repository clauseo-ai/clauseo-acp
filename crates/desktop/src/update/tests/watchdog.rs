//! The install's watchdog (`install::watch_install`, `Watch::INSTALL`) and the install skipped
//! when an agents job outlived its bound at quit.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use clauseo_host::{Downloaded, HostExit, UpdaterError};

use super::super::install::{self, After, Steps, Watch, Watched};
use super::{Recorded, downloaded};

/// A short watch for the tests: 50 ms, then up to 3 looks 10 ms apart.
const SHORT: Watch = Watch {
    bound: Duration::from_millis(50),
    poll: Duration::from_millis(10),
    polls: 3,
};

/// An install that answers after `after` (never, with `None`; its thread holds the sender open
/// until `release` is dropped).
fn slow_install(
    after: Option<Duration>,
) -> (
    Receiver<Result<(), UpdaterError>>,
    std::sync::mpsc::Sender<()>,
) {
    let (answer, answered) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        match after {
            Some(after) => drop(released.recv_timeout(after)),
            None => drop(released.recv()),
        }
        if after.is_some() {
            drop(answer.send(Ok(())));
        }
    });
    (answered, release)
}

/// The watchdog: an install that returns within the bound is heard; one that hangs is given up
/// on once the bound passes, with the app in place; one whose thread died is lost.
#[test]
fn a_hung_install_is_given_up_on_after_its_bound() {
    let long = Watch {
        bound: crate::test_support::STEP_WAIT,
        ..SHORT
    };
    let (answer, _release) = slow_install(Some(Duration::from_millis(1)));
    assert_eq!(
        install::watch_install(&answer, long, || true),
        Watched::Returned(Ok(()))
    );

    let (answer, release) = slow_install(None);
    let looked = std::cell::Cell::new(0);
    let in_place = || {
        looked.set(looked.get() + 1);
        true
    };
    assert_eq!(
        install::watch_install(&answer, SHORT, in_place),
        Watched::GaveUp
    );
    assert_eq!(looked.get(), 1, "gave up at the first look past the bound");
    drop(release);

    let (answer, answered) = std::sync::mpsc::channel::<Result<(), UpdaterError>>();
    drop(answer);
    assert_eq!(
        install::watch_install(&answered, SHORT, || true),
        Watched::Lost
    );
}

/// While the app is not in place (a swap between its two moves) the watchdog keeps waiting,
/// and hears an install that returns then; it gives up after its last look all the same.
#[test]
fn the_watchdog_waits_while_the_app_is_not_in_place() {
    // Answers after the bound (20 ms), well within the looks (500 × 20 ms).
    let looks = Watch {
        bound: Duration::from_millis(20),
        poll: Duration::from_millis(20),
        polls: 500,
    };
    let (answer, _release) = slow_install(Some(Duration::from_millis(100)));
    assert_eq!(
        install::watch_install(&answer, looks, || false),
        Watched::Returned(Ok(()))
    );

    let (answer, release) = slow_install(None);
    let looked = std::cell::Cell::new(0);
    let not_in_place = || {
        looked.set(looked.get() + 1);
        false
    };
    assert_eq!(
        install::watch_install(&answer, SHORT, not_in_place),
        Watched::GaveUp
    );
    assert_eq!(looked.get(), 3);
    drop(release);
}

/// A slow install injected into `after_host`: given up on after the bound, logged, and the app
/// restarts as it would after a failed one, so the next start's marker reads `failed`.
#[test]
fn a_hung_install_restarts_the_app() {
    struct Hangs(Recorded, Option<std::sync::mpsc::Sender<()>>);
    impl Steps for Hangs {
        fn installable(&self, downloaded: &Downloaded) -> bool {
            self.0.installable(downloaded)
        }
        fn save_window_state(&mut self) {
            self.0.save_window_state();
        }
        fn crash_clean_exit(&mut self) {
            self.0.crash_clean_exit();
        }
        fn flush_log(&mut self) {
            self.0.flush_log();
        }
        fn start_install(&mut self, _downloaded: Downloaded) -> Receiver<Result<(), UpdaterError>> {
            self.0.steps.push("install");
            let (answer, release) = slow_install(None);
            self.1 = Some(release);
            answer
        }
        fn app_in_place(&self) -> bool {
            true
        }
    }
    let mut steps = Hangs(Recorded::new(true, Ok(())), None);
    let after = install::after_host(HostExit::Install(downloaded()), true, &mut steps, SHORT);
    assert_eq!(after, After::Restart);
    assert_eq!(
        steps.0.steps,
        [
            "window state",
            "crash clean exit",
            "flush",
            "install",
            "flush"
        ]
    );
}

/// Without the agents stopped, nothing is installed: the window state is saved and the app
/// restarts as the old version.
#[test]
fn an_agents_job_still_running_installs_nothing_and_restarts() {
    let mut steps = Recorded::new(true, Ok(()));
    let after = install::after_host(HostExit::Install(downloaded()), false, &mut steps, SHORT);
    assert_eq!(after, After::Restart);
    assert_eq!(steps.steps, ["window state", "flush"]);
}

/// The install's watch is the 5 minutes, then the minute of looks, that `Watch::INSTALL`
/// explains.
#[test]
fn the_install_s_watch_is_five_minutes_and_a_minute_of_looks() {
    assert_eq!(Watch::INSTALL.bound, Duration::from_secs(300));
    assert_eq!(
        Watch::INSTALL.poll * Watch::INSTALL.polls,
        Duration::from_secs(60)
    );
}
