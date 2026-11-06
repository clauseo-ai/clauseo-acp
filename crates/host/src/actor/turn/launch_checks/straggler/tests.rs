//! The removal's wait: Chrome's debugging port answering keeps it, a refused one ends it, and
//! the cap says what was still there.

use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

use std::sync::Arc;

use clauseo_protocol::{MatterId, ThreadId};

use super::{Budget, CONNECT_WAIT, HeldRuns, POLL, STOP_GRACE, Straggler, wait_gone};
use crate::actor::turn::launch_checks::RunFolders;
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;

/// A session's browser claims, its socket folder (Unix) where production puts it, removed as
/// it drops.
fn claims(test: &str) -> BrowserSession {
    BrowserSession::claim(
        test,
        #[cfg(unix)]
        None,
    )
    .unwrap()
}

/// A listener on the session's debugging port, as Chrome's browser process is.
fn chrome(session: &BrowserSession) -> TcpListener {
    TcpListener::bind((Ipv4Addr::LOCALHOST, session.ports.cdp.port())).unwrap()
}

#[test]
fn a_listener_on_the_debugging_port_holds_the_wait_to_its_cap() {
    let session = claims("held");
    let listener = chrome(&session);
    let waited = wait_gone(None, Some(&session), Duration::from_millis(300), || false);
    assert_eq!(waited, Err(Straggler::Chrome));
    drop(listener);
    assert_eq!(
        wait_gone(None, Some(&session), Duration::ZERO, || false),
        Ok(())
    );
}

#[test]
fn the_wait_ends_once_chrome_stops_listening() {
    let session = claims("ends");
    let listener = chrome(&session);
    let (close, closing) = std::sync::mpsc::channel::<()>();
    let closer = std::thread::spawn(move || {
        // Chrome goes once the wait has seen it at least once.
        closing.recv().unwrap();
        drop(listener);
    });
    assert_eq!(
        wait_gone(None, Some(&session), Duration::from_millis(100), || false),
        Err(Straggler::Chrome)
    );
    close.send(()).unwrap();
    assert_eq!(
        wait_gone(None, Some(&session), Duration::from_secs(10), || false),
        Ok(())
    );
    closer.join().unwrap();
}

#[test]
fn no_tree_and_no_browser_is_gone_at_once() {
    assert_eq!(wait_gone(None, None, Duration::ZERO, || false), Ok(()));
    let held = HeldRuns::default();
    assert_eq!(held.release_all(), 0);
}

/// Polls until `budget` is spent, each costing one park and, when `slow`, a timed-out connect;
/// answers the real time they would take.
fn time_until_spent(mut budget: Budget, slow: bool) -> Duration {
    let mut took = Duration::ZERO;
    loop {
        if slow {
            budget.charge(Budget::CONNECT_POLLS);
            took += CONNECT_WAIT;
        }
        if budget.spent() {
            return took;
        }
        budget.charge(1);
        took += POLL;
    }
}

/// Timed-out connects are charged their time, so a wait of slow looks stays within its cap plus
/// one look, as a wait of quick ones does, and not five times past it.
#[test]
fn the_budget_charges_a_timed_out_connect() {
    let cap = Duration::from_secs(20);
    for slow in [false, true] {
        let took = time_until_spent(Budget::new(cap), slow);
        assert!(took >= cap.saturating_sub(CONNECT_WAIT), "{slow}: {took:?}");
        assert!(took <= cap + POLL + CONNECT_WAIT, "{slow}: {took:?}");
    }
}

/// Once the host is stopping, a wait gives up within the grace, however long its cap.
#[test]
fn a_stop_ends_the_wait_within_the_grace() {
    let session = claims("stopping");
    let listener = chrome(&session);
    assert_eq!(
        wait_gone(None, Some(&session), Duration::from_secs(600), || true),
        Err(Straggler::Chrome)
    );
    let mut budget = Budget::new(Duration::from_secs(600));
    budget.end_within(STOP_GRACE);
    assert!(time_until_spent(budget, false) <= STOP_GRACE + POLL);
    drop(listener);
}

/// After the host's stop released the held runs, a run that comes to be held drops at once.
#[test]
fn a_hold_after_the_release_drops_the_run_at_once() {
    let held = HeldRuns::default();
    let session = Arc::new(claims("after-stop"));
    let alive = Arc::downgrade(&session);
    let key = SessionKey {
        matter: id::<MatterId>(1),
        thread: id::<ThreadId>(2),
        launch: 3,
    };
    let mut run = RunFolders::new(&std::env::temp_dir(), key);
    run.browser_session = Some(session);
    held.begin_stop();
    assert!(held.stopping());
    assert_eq!(held.release_all(), 0);
    assert!(!held.hold(run), "nobody is left to free it");
    assert!(alive.upgrade().is_none(), "its claims went at once");
    assert_eq!(held.release_all(), 0);
}
