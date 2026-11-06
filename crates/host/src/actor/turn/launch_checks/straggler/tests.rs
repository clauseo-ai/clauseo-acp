//! The removal's wait: Chrome's debugging port answering keeps it, a refused one ends it, and
//! the cap says what was still there.

use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

use super::{HeldRuns, Straggler, wait_gone};
use crate::browser::session::BrowserSession;

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
    let waited = wait_gone(None, Some(&session), Duration::from_millis(300));
    assert_eq!(waited, Err(Straggler::Chrome));
    drop(listener);
    assert_eq!(wait_gone(None, Some(&session), Duration::ZERO), Ok(()));
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
        wait_gone(None, Some(&session), Duration::from_millis(100)),
        Err(Straggler::Chrome)
    );
    close.send(()).unwrap();
    assert_eq!(
        wait_gone(None, Some(&session), Duration::from_secs(10)),
        Ok(())
    );
    closer.join().unwrap();
}

#[test]
fn no_tree_and_no_browser_is_gone_at_once() {
    assert_eq!(wait_gone(None, None, Duration::ZERO), Ok(()));
    let held = HeldRuns::default();
    assert_eq!(held.release_all(), 0);
}
