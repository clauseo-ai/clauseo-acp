//! Offline retries: a try made from `Waiting` stays `Waiting` until the network answers, and a
//! connectivity hint (a `FakeWatch`) starts the next try 2 s later instead of at the next 30 s
//! retry; a server's error status ends `Waiting` too. Time is the fixture's manual timer; the
//! only real-time bound is a ceiling on a wait that should be armed, so a broken rule fails at
//! once rather than hanging the test.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::testing::{Behaviour, FakeWatch};
use clauseo_agents::{FailureKind, InstallState, TestResolver};

use crate::support::{Fixture, agent_a, tarball_path};

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// The ceiling on waiting for a wait the job should arm at once.
const BOUND: Duration = Duration::from_secs(10);

fn is_waiting(state: &InstallState) -> bool {
    matches!(
        state,
        InstallState::Waiting {
            reason: FailureKind::Offline,
            ..
        }
    )
}

fn is_downloading(state: &InstallState) -> bool {
    matches!(state, InstallState::Downloading { .. })
}

/// A fixture that is offline until the flag is set, with a `FakeWatch` for its hints.
fn offline_fixture(label: &str) -> (Fixture, Arc<AtomicBool>, FakeWatch) {
    let mut fx = Fixture::new(label);
    let online = Arc::new(AtomicBool::new(false));
    let addr = fx.registry.addr();
    let flag = Arc::clone(&online);
    fx.config.install.resolver = Some(TestResolver(Arc::new(move |name: &str| {
        if flag.load(Ordering::SeqCst) {
            Ok(vec![addr])
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no {name}"),
            ))
        }
    })));
    let watch = FakeWatch::new();
    fx.config.connectivity = Arc::new(watch.clone());
    (fx, online, watch)
}

/// The resolver's modes in [`a_retry_from_waiting_stays_waiting_until_the_network_answers`].
const FAIL: u8 = 0;
const HANG: u8 = 1;
const ONLINE: u8 = 2;

/// A retry whose lookup hangs and then fails sends no `Downloading` in between: the state stays
/// `Waiting`. The retry that gets through sends `Downloading` once bytes arrive.
#[test]
fn a_retry_from_waiting_stays_waiting_until_the_network_answers() {
    let mut fx = Fixture::new("waiting-hang");
    let mode = Arc::new(AtomicU8::new(FAIL));
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (entered_tx, release_rx) = (Mutex::new(entered_tx), Mutex::new(release_rx));
    let addr = fx.registry.addr();
    let current = Arc::clone(&mode);
    fx.config.install.resolver = Some(TestResolver(Arc::new(move |name: &str| {
        match current.load(Ordering::SeqCst) {
            ONLINE => return Ok(vec![addr]),
            HANG => {
                // The connect hangs until the test lets it fail.
                entered_tx.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
            _ => {}
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no {name}"),
        ))
    })));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);

    mode.store(HANG, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    entered_rx.recv().unwrap();
    mode.store(FAIL, Ordering::SeqCst);
    release_tx.send(()).unwrap();
    let retry = events.until(&agent_a(), is_waiting);
    assert!(
        !retry.iter().any(|(_, s)| is_downloading(s)),
        "no Downloading while the try hung: {retry:?}"
    );

    mode.store(ONLINE, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(
        seen.iter().any(|(_, s)| is_downloading(s)),
        "the try that got through sent Downloading: {seen:?}"
    );
    assert!(agents.shutdown(secs(10)));
}

/// A hint while `Waiting` starts a try after the 2 s debounce, well before the 30 s retry.
#[test]
fn a_hint_while_waiting_starts_a_try_after_two_seconds() {
    let (fx, online, watch) = offline_fixture("hint-try");
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);
    assert_eq!(watch.live(), 1, "subscribed while Waiting");

    online.store(true, Ordering::SeqCst);
    assert_eq!(watch.change(), 1);
    assert!(
        fx.timer.fire_next_within(secs(2), BOUND),
        "a hint arms the 2 s debounce"
    );
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    // The 30 s retry was armed and never fired: the hint's try installed.
    assert_eq!(fx.timer.install_waits(), [secs(30), secs(2)]);
    assert_eq!(watch.live(), 0, "unsubscribed at the end");
    assert!(agents.shutdown(secs(10)));
}

/// Three hints inside the debounce give one try: once it fails, the job waits 30 s with no
/// second debounce.
#[test]
fn three_hints_within_a_second_give_one_try() {
    let (fx, online, watch) = offline_fixture("hint-burst");
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);

    for _ in 0..3 {
        assert_eq!(watch.change(), 1);
    }
    assert!(
        fx.timer.fire_next_within(secs(2), BOUND),
        "the hints arm the 2 s debounce"
    );
    // Still offline: the one try fails and waits again.
    events.until(&agent_a(), is_waiting);
    online.store(true, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    // A hint left over would have armed a second 2 s debounce with the second 30 s wait.
    assert_eq!(fx.timer.install_waits(), [secs(30), secs(2), secs(30)]);
    assert!(agents.shutdown(secs(10)));
}

/// Once `Downloading` is out the job has left `Waiting`: unsubscribed, so a hint reaches
/// nothing and arms no debounce.
#[test]
fn a_hint_after_leaving_waiting_has_no_effect() {
    let (fx, online, watch) = offline_fixture("hint-after");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);
    assert_eq!(watch.live(), 1);

    online.store(true, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    fx.registry.wait_for_stalls(1);
    events.until(&agent_a(), is_downloading);
    assert_eq!(watch.live(), 0, "unsubscribed on leaving Waiting");
    assert_eq!(watch.change(), 0, "the hint reached no job");

    fx.registry.release();
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.timer.install_waits(), [secs(30)]);
    assert_eq!(watch.made(), 1);
    assert!(agents.shutdown(secs(10)));
}

/// A shutdown while `Waiting` ends the subscription.
#[test]
fn shutdown_while_waiting_unsubscribes() {
    let (fx, _online, watch) = offline_fixture("hint-shutdown");
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);
    assert_eq!(watch.live(), 1);
    assert!(agents.shutdown(secs(10)));
    assert_eq!(watch.live(), 0);
}

/// A server answering 503 while `Waiting` ends `Waiting`: the network is there. The job sends
/// `Downloading`, drops its subscription, and retries after its 2 s back-off.
#[test]
fn an_error_status_while_waiting_ends_waiting() {
    let (fx, online, watch) = offline_fixture("waiting-503");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Status(503));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);

    online.store(true, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    // The 503's try is over once its back-off is armed: every state it sent is queued.
    assert!(
        fx.timer.armed_within(secs(2), BOUND),
        "the 503 is a counted try with a back-off"
    );
    let first = fx.registry.requests().remove(0);
    assert_eq!(
        first.path,
        tarball_path("adapter-a"),
        "the 503 was the first answer"
    );
    let after = events.drain();
    assert!(
        after.iter().any(|(_, s)| is_downloading(s)),
        "Waiting ended: {after:?}"
    );
    assert!(!after.iter().any(|(_, s)| is_waiting(s)), "{after:?}");
    assert_eq!(watch.live(), 0, "unsubscribed on leaving Waiting");

    assert!(fx.timer.fire_read());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.requests_for("adapter-a").len(), 2);
    assert!(agents.shutdown(secs(10)));
}

/// The job subscribes before `Waiting` goes out, so a hint the UI's `Waiting` could prompt is
/// never missed. The fake holds the job in its subscription while the test looks.
#[test]
fn the_job_subscribes_before_waiting_goes_out() {
    let (fx, _online, watch) = offline_fixture("subscribe-order");
    let (at_tx, at_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (at_tx, go_rx) = (Mutex::new(at_tx), Mutex::new(go_rx));
    watch.on_subscribe(move || {
        at_tx.lock().unwrap().send(()).unwrap();
        go_rx.lock().unwrap().recv().unwrap();
    });
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    at_rx.recv().unwrap();
    // The job is held in its subscription: every state it has sent is queued.
    let before = events.drain();
    assert!(
        !before.iter().any(|(_, s)| is_waiting(s)),
        "Waiting went out before the subscription: {before:?}"
    );
    go_tx.send(()).unwrap();
    events.until(&agent_a(), is_waiting);
    assert_eq!(watch.live(), 1);
    assert!(agents.shutdown(secs(10)));
}

/// A pause during the 2 s debounce pauses the job at once, with no try.
#[test]
fn a_pause_during_the_debounce_pauses_with_no_try() {
    let (fx, online, watch) = offline_fixture("hint-pause");
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), is_waiting);

    // Online now, so a try would reach the registry.
    online.store(true, Ordering::SeqCst);
    assert_eq!(watch.change(), 1);
    assert!(
        fx.timer.armed_within(secs(2), BOUND),
        "the hint arms the debounce"
    );
    agents.pause(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Paused));
    // Neither the debounce nor the 30 s wait is ended: the pause alone ends the job.
    assert!(agents.shutdown(secs(10)), "the job ended at the pause");
    assert!(fx.registry.requests().is_empty(), "no try");
    assert_eq!(watch.live(), 0);
}
