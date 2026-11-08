//! `RecheckBrowserPolicy` through the host: the lawyer's "Try again" on a browser whose policy
//! could not be read. It takes the path a Chrome download's end takes: the read's result reaches
//! every app subscriber (`HostEvent::BrowserPolicy`), the next session started gets the browser
//! when nothing blocks it, and a running session's header keeps saying what that session has,
//! except that a policy now found blocking is named so. One read at a time.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use clauseo_host::AppSubscription;
use clauseo_protocol::ToolsUnavailableReason::{BrowserBlockedByPolicy, BrowserPolicyUnreadable};
use clauseo_protocol::{BrowserPolicyState, CommandOutcome, HostCommand, HostEvent};
use clauseo_settings::chromium_policy::{Found, RemoteDebugging, RemoteDebuggingPolicy};
use futures::StreamExt as _;

use super::{Chrome, Rig, Says};
use crate::browser_install::subscribe;
use crate::support::wait;

/// A firm policy that says each of `said` in turn, at Edge's user place, then the last one from
/// then on. With a gate, every read after the first (the first launch's) waits for a release.
#[derive(Debug)]
struct Turns {
    said: Vec<Found>,
    reads: AtomicUsize,
    gate: Option<Mutex<mpsc::Receiver<()>>>,
}

impl Turns {
    fn new(said: Vec<Found>) -> Arc<Self> {
        Arc::new(Self {
            said,
            reads: AtomicUsize::new(0),
            gate: None,
        })
    }

    /// Like [`Turns::new`], with the gate: each send releases one read.
    fn gated(said: Vec<Found>) -> (Arc<Self>, mpsc::Sender<()>) {
        let (release, gate) = mpsc::channel();
        let turns = Self {
            said,
            reads: AtomicUsize::new(0),
            gate: Some(Mutex::new(gate)),
        };
        (Arc::new(turns), release)
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl RemoteDebuggingPolicy for Turns {
    fn read(&self) -> RemoteDebugging {
        let n = self.reads.fetch_add(1, Ordering::SeqCst);
        if n > 0
            && let Some(gate) = &self.gate
        {
            let _released = gate.lock().unwrap().recv();
        }
        let said = self.said[n.min(self.said.len() - 1)].clone();
        Says(said).read()
    }
}

fn unreadable() -> Found {
    Found::Unreadable("the registry read failed (5)".to_owned())
}

const RECHECK: HostCommand = HostCommand::RecheckBrowserPolicy {};

/// The policy states on `sub` up to the first that ends a read (any but `Checking`), skipping
/// other events.
fn states(sub: &mut AppSubscription) -> Vec<BrowserPolicyState> {
    wait("the policy states", async {
        let mut states = Vec::new();
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::BrowserPolicy { state } = crate::settings::decode(&item) {
                states.push(state);
                if state != BrowserPolicyState::Checking {
                    return states;
                }
            }
        }
    })
}

/// A read error, then a clean read: the app hears `Clear` (a fresh subscription too, in its
/// catch-up), the running session's header still says the policy was not read (it has no
/// browser), and the next session gets the browser, as after a Chrome download.
#[test]
fn a_clean_reread_clears_for_the_next_session() {
    let turns = Turns::new(vec![unreadable(), Found::Absent]);
    let rig = Rig::start("recheck_clear", turns, None, Chrome::Forced);
    let (first, reason) = rig.launch();
    assert_eq!(reason, Some(BrowserPolicyUnreadable));
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(
        states(&mut subscribe(&rig.host)),
        [BrowserPolicyState::Clear]
    );
    assert_eq!(rig.browser(first), Some(BrowserPolicyUnreadable));
    assert_eq!(rig.launch().1, None);
    rig.shutdown();
}

/// A read that fails again: the app hears `Unreadable`, and the header stays as it was.
#[test]
fn a_reread_that_fails_again_changes_nothing() {
    let rig = Rig::start(
        "recheck_unreadable",
        Turns::new(vec![unreadable()]),
        None,
        Chrome::Forced,
    );
    let (first, _) = rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Unreadable]
    );
    assert_eq!(rig.browser(first), Some(BrowserPolicyUnreadable));
    rig.shutdown();
}

/// A read that finds a false: the app hears `Blocked`, and the header that said the policy was
/// not read now says the firm's policy blocks the browser.
#[test]
fn a_reread_that_finds_a_false_names_the_block() {
    let rig = Rig::start(
        "recheck_blocked",
        Turns::new(vec![unreadable(), Found::Disallowed]),
        None,
        Chrome::Forced,
    );
    let (first, _) = rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Blocked]
    );
    assert_eq!(rig.browser(first), Some(BrowserBlockedByPolicy));
    rig.shutdown();
}

/// A second "Try again" while a read runs answers `Done` and joins it: one read, one
/// `Checking`. Once it has ended, the next one reads again.
#[test]
fn two_rechecks_in_flight_read_once() {
    let (turns, release) = Turns::gated(vec![unreadable(), Found::Absent]);
    let rig = Rig::start(
        "recheck_joined",
        Arc::clone(&turns) as Arc<dyn RemoteDebuggingPolicy>,
        None,
        Chrome::Forced,
    );
    rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    release.send(()).unwrap();
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(turns.reads(), 2, "the launch's read and one re-read");
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    release.send(()).unwrap();
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(turns.reads(), 3);
    rig.shutdown();
}

/// Once the app has heard `Clear`, a later launch whose read fails again keeps it current: the
/// app hears `Unreadable`, so no row says the browser is ready when it is not.
#[test]
fn a_later_launch_read_error_overrides_a_clear() {
    let rig = Rig::start(
        "recheck_then_launch",
        Turns::new(vec![unreadable(), Found::Absent, unreadable()]),
        None,
        Chrome::Forced,
    );
    rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(rig.launch().1, Some(BrowserPolicyUnreadable));
    assert_eq!(states(&mut sub), [BrowserPolicyState::Unreadable]);
    rig.shutdown();
}
