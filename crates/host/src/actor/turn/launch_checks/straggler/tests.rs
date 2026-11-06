//! The removal's wait: Chrome's debugging port answering keeps it, a refused one ends it, and
//! the cap says what was still there.

use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

use std::sync::Arc;

use clauseo_protocol::{MatterId, ThreadId};

#[cfg(windows)]
use super::still_there;
use super::{
    Budget, CONNECT_WAIT, HeldRuns, POLL, Parts, Probe, STOP_GRACE, Straggler, chrome_alive,
    probe_for, wait_gone,
};
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

/// After the host's stop released the held runs, parts that come to be held drop at once.
#[test]
fn a_hold_after_the_release_drops_the_parts_at_once() {
    let held = HeldRuns::default();
    let session = Arc::new(claims("after-stop"));
    let alive = Arc::downgrade(&session);
    held.begin_stop();
    assert!(held.stopping());
    assert_eq!(held.release_all(), 0);
    assert!(held.closed());
    let parts = Parts {
        browser: Some(session),
        ..Parts::default()
    };
    assert!(!held.hold(parts), "nobody is left to free them");
    assert!(alive.upgrade().is_none(), "the claims went at once");
    assert_eq!(held.count(), 0);
}

/// A run's lease: dropped without a completed wait it holds the run's parts; after
/// `wait_done` it frees them; and every clone of the run shares it.
#[test]
fn the_lease_frees_only_after_a_completed_wait() {
    let key = SessionKey {
        matter: id::<MatterId>(1),
        thread: id::<ThreadId>(2),
        launch: 3,
    };
    let held = HeldRuns::default();
    for done in [false, true] {
        let session = Arc::new(claims(if done { "lease-done" } else { "lease-held" }));
        let alive = Arc::downgrade(&session);
        let run = RunFolders::new(&std::env::temp_dir(), key);
        run.lease.keep(
            held.clone(),
            Parts {
                browser: Some(session),
                ..Parts::default()
            },
        );
        let clone = run.clone();
        drop(run);
        assert!(alive.upgrade().is_some(), "a clone still holds the run");
        if done {
            clone.lease.wait_done();
        }
        drop(clone);
        assert_eq!(alive.upgrade().is_some(), !done, "done: {done}");
    }
    assert_eq!(held.count(), 1, "the run without a completed wait is held");
    assert_eq!(held.release_all(), 1);
}

/// The short look sees a listener as there and an empty port as gone, on every platform.
#[test]
fn the_short_look_counts_only_an_accept() {
    let session = claims("short-look");
    let listener = chrome(&session);
    assert!(chrome_alive(&session, Probe::Accepting).is_some());
    drop(listener);
    assert!(chrome_alive(&session, Probe::Accepting).is_none());
}

/// The short look is Windows', after a gone tree that held the session's Chrome; with no tree,
/// or a Chrome started elsewhere, and on Unix always, the full probe.
#[test]
fn the_short_look_follows_a_tree_that_held_chrome() {
    let mut session = claims("probe-for");
    assert!(session.chrome_in_tree);
    let short = if cfg!(windows) {
        Probe::Accepting
    } else {
        Probe::Full
    };
    assert_eq!(probe_for(true, &session), short);
    assert_eq!(probe_for(false, &session), Probe::Full);
    session.chrome_in_tree = false;
    assert_eq!(probe_for(true, &session), Probe::Full);
}

/// Windows: an agent tree, run to its end and gone.
#[cfg(windows)]
fn gone_tree() -> clauseo_process::TreeWatch {
    use clauseo_process::{LaunchSpec, Launched, SpawnOptions, spawn_in_tree};

    let cmd = std::env::var_os("ComSpec").unwrap();
    let spec = LaunchSpec::new(
        cmd.into(),
        vec!["/c".into(), "exit 0".into()],
        std::collections::BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    let Launched { mut handle, .. } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    let watch = handle.tree_watch();
    futures::executor::block_on(handle.exited());
    drop(handle);
    watch
}

/// Windows: after a gone tree, a listener on the port (a Chrome started outside the job, by
/// WMI or the Task Scheduler) still holds the wait, and an empty port is gone at the first
/// look, with no wait for Windows' slow refusal (a zero cap allows one look).
#[cfg(windows)]
#[test]
fn after_a_gone_tree_only_a_listener_holds_on_windows() {
    let session = claims("tree-gone");
    let tree = gone_tree();
    let listener = chrome(&session);
    let cap = Duration::from_millis(300);
    assert_eq!(
        wait_gone(Some(&tree), Some(&session), cap, || false),
        Err(Straggler::Chrome)
    );
    drop(listener);
    assert_eq!(
        wait_gone(Some(&tree), Some(&session), Duration::ZERO, || false),
        Ok(())
    );
    assert!(still_there(Some(&tree), Some(&session)).is_none());
}

/// Windows: a Chrome started outside the agent's tree gets the full probe after the tree is
/// gone: a listener holds, and an empty port reads gone once Windows refuses.
#[cfg(windows)]
#[test]
fn a_chrome_outside_the_tree_gets_the_full_probe_on_windows() {
    let mut session = claims("outside-tree");
    session.chrome_in_tree = false;
    let tree = gone_tree();
    let listener = chrome(&session);
    assert_eq!(
        wait_gone(
            Some(&tree),
            Some(&session),
            Duration::from_millis(300),
            || false
        ),
        Err(Straggler::Chrome)
    );
    drop(listener);
    assert_eq!(
        wait_gone(Some(&tree), Some(&session), Duration::from_secs(10), || {
            false
        }),
        Ok(())
    );
}
