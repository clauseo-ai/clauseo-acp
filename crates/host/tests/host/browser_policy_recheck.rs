//! `RecheckBrowserPolicy` through the host: the lawyer's "Try again" on a browser whose policy
//! could not be read. It takes the path a Chrome download's end takes: the read's result reaches
//! every app subscriber (`HostEvent::BrowserPolicy`), the next session started gets the browser
//! when nothing blocks it, and a running session's header keeps saying what that session has,
//! except that a policy now found blocking is named so. One read at a time, at most 10 s on the
//! host clock.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_host::AppSubscription;
use clauseo_protocol::ToolsUnavailableReason::{BrowserBlockedByPolicy, BrowserPolicyUnreadable};
use clauseo_protocol::{BrowserPolicyState, CommandOutcome, HostCommand, HostEvent};
use clauseo_settings::chromium_policy::{Found, RemoteDebugging, RemoteDebuggingPolicy};
use futures::StreamExt as _;

use super::{Chrome, Rig, Says};
use crate::browser_install::subscribe;
use crate::support::wait;

/// A firm policy that says each of `said` in turn, at Edge's user place, then the last one from
/// then on. Read `n` (from 1; read 0 is the first launch's) waits for gate `n - 1` when there is
/// one, so a test holds each read and lets it end in the order it wants.
#[derive(Debug)]
struct Turns {
    said: Vec<Found>,
    reads: AtomicUsize,
    /// Each read's number as it starts (`false`) and as it ends (`true`).
    seen: Mutex<mpsc::Sender<(usize, bool)>>,
    gates: Vec<Mutex<mpsc::Receiver<()>>>,
}

/// The receiver of each read's number as it starts (`false`) and ends (`true`).
type Seen = mpsc::Receiver<(usize, bool)>;

impl Turns {
    fn new(said: Vec<Found>) -> Arc<Self> {
        Self::gated(said, 0).0
    }

    /// Like [`Turns::new`], with `gates` gates: the sender at `i` releases read `i + 1`.
    fn gated(said: Vec<Found>, gates: usize) -> (Arc<Self>, Vec<mpsc::Sender<()>>) {
        let (turns, releases, _ends) = Self::watched(said, gates);
        (turns, releases)
    }

    /// Like [`Turns::gated`], with the receiver of each read's number as it starts and ends.
    fn watched(said: Vec<Found>, gates: usize) -> (Arc<Self>, Vec<mpsc::Sender<()>>, Seen) {
        let (releases, gates) = (0..gates)
            .map(|_| mpsc::channel())
            .unzip::<_, _, Vec<_>, Vec<_>>();
        let (seen, ends) = mpsc::channel();
        let turns = Self {
            said,
            reads: AtomicUsize::new(0),
            seen: Mutex::new(seen),
            gates: gates.into_iter().map(Mutex::new).collect(),
        };
        (Arc::new(turns), releases, ends)
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl RemoteDebuggingPolicy for Turns {
    fn read(&self) -> RemoteDebugging {
        let n = self.reads.fetch_add(1, Ordering::SeqCst);
        let _unheard = self.seen.lock().unwrap().send((n, false));
        if let Some(gate) = n.checked_sub(1).and_then(|at| self.gates.get(at)) {
            let _released = gate.lock().unwrap().recv();
        }
        let said = self.said[n.min(self.said.len() - 1)].clone();
        let read = Says(said).read();
        let _unheard = self.seen.lock().unwrap().send((n, true));
        read
    }
}

fn unreadable() -> Found {
    Found::Unreadable("the registry read failed (5)".to_owned())
}

const RECHECK: HostCommand = HostCommand::RecheckBrowserPolicy {};

/// The next policy state on `sub`, skipping other events.
fn next_state(sub: &mut AppSubscription) -> BrowserPolicyState {
    wait("a policy state", async {
        loop {
            let item = sub.next().await.expect("the app stream ended");
            if let HostEvent::BrowserPolicy { state } = crate::settings::decode(&item) {
                return state;
            }
        }
    })
}

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
    let (turns, release) = Turns::gated(vec![unreadable(), Found::Absent], 2);
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
    release[0].send(()).unwrap();
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(turns.reads(), 2, "the launch's read and one re-read");
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    release[1].send(()).unwrap();
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

/// A read that hangs ends as `Unreadable` once 10 s pass on the host clock, and the next "Try
/// again" starts a new read; the hung read's late answer (a false here) is dropped.
#[test]
fn a_hung_reread_times_out_as_unreadable_and_its_late_answer_is_dropped() {
    let said = vec![unreadable(), Found::Disallowed, Found::Absent];
    let (turns, release, ends) = Turns::watched(said, 2);
    let rig = Rig::start(
        "recheck_hung",
        Arc::clone(&turns) as Arc<dyn RemoteDebuggingPolicy>,
        None,
        Chrome::Forced,
    );
    let (first, _) = rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    // The deadline is armed in the step that publishes `Checking`.
    assert_eq!(next_state(&mut sub), BrowserPolicyState::Checking);
    rig.host.clock.advance(Duration::from_secs(10));
    assert_eq!(states(&mut sub), [BrowserPolicyState::Unreadable]);
    // A new read starts while the hung one still runs (once that one holds gate 0); it ends
    // first, clear.
    let late = Duration::from_secs(20);
    while ends.recv_timeout(late).expect("the hung read starts") != (1, false) {}
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    release[1].send(()).unwrap();
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    // The hung read answers late, with a false: nothing follows it.
    release[0].send(()).unwrap();
    while ends.recv_timeout(late).expect("the hung read ends") != (1, true) {}
    assert_eq!(turns.reads(), 3);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Clear]
    );
    assert_eq!(rig.browser(first), Some(BrowserPolicyUnreadable));
    rig.shutdown();
}

/// Once the app has heard `Unreadable`, a later launch that reads the policy clean keeps it
/// current: the app hears `Clear`.
#[test]
fn a_later_clean_launch_read_overrides_unreadable() {
    let rig = Rig::start(
        "recheck_then_clean_launch",
        Turns::new(vec![unreadable(), unreadable(), Found::Absent]),
        None,
        Chrome::Forced,
    );
    rig.launch();
    let mut sub = subscribe(&rig.host);
    assert_eq!(rig.host.ok(RECHECK), CommandOutcome::Done);
    assert_eq!(
        states(&mut sub),
        [BrowserPolicyState::Checking, BrowserPolicyState::Unreadable]
    );
    assert_eq!(rig.launch().1, None);
    assert_eq!(states(&mut sub), [BrowserPolicyState::Clear]);
    rig.shutdown();
}
