//! Desktop's launch gate (`HostConfig::with_launch_gate`, R17 M3): asked on the launch-prep
//! thread before each launch. The tests play desktop: before its gate answers a refusal it sets
//! that refusal itself through `set_launch_refused`, and the host only acts on the answer. A
//! waited-out answer (`agent.identity_checking`) waits for the refusal to lift, once, woken early
//! by the lift; another refusal fails the turn with its code; a code that is no launch refusal,
//! or a panic, fails closed with `agent.launch_failed`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clauseo_host::{Clock as _, HostHandle, LaunchGate};
use clauseo_protocol::{AgentId, ErrorCode, StopReason, StreamId, ThreadId, TurnId};

use crate::codex::{Recording, turn_failed};
use crate::keys_loading::{KEYS_WAIT, fake, until_keys_wait_armed};
use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// What desktop's identity check takes in test (a): well under the host's 2 s wait.
const CHECK_TAKES: Duration = Duration::from_millis(300);

/// A host with `gate`, desktop's handle for the gate once started, and the launches made.
struct Gated {
    _dir: Dir,
    host: Running,
    thread: ThreadId,
    events: Events,
    launches: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
    retries: Arc<Mutex<Vec<(AgentId, ErrorCode)>>>,
}

/// Starts a host whose launch gate is `gate`, given desktop's handle (set once the host runs).
fn gated(
    test: &str,
    gate: impl Fn(&HostHandle, &AgentId) -> Option<ErrorCode> + Send + Sync + 'static,
) -> Gated {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local("launch-refused"))]);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let handle: Arc<OnceLock<HostHandle>> = Arc::default();
    let desktop = Arc::clone(&handle);
    let launch_gate: LaunchGate =
        Arc::new(move |agent: &AgentId| gate(desktop.get().expect("the host runs"), agent));
    let retries: Arc<Mutex<Vec<(AgentId, ErrorCode)>>> = Arc::default();
    let asked = Arc::clone(&retries);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config
            .with_launch_gate(launch_gate)
            .with_launch_retry(Arc::new(move |agent: &AgentId, code: ErrorCode| {
                asked.lock().unwrap().push((agent.clone(), code));
            }))
    });
    assert!(handle.set(host.handle.clone()).is_ok());
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let events = host.events(StreamId::Thread(thread));
    Gated {
        _dir: dir,
        host,
        thread,
        events,
        launches,
        retries,
    }
}

/// Desktop's side of a refusing gate: sets `code` on the agent's launches, and awaits it.
fn refuse(handle: &HostHandle, code: Option<ErrorCode>) {
    wait(
        "set_launch_refused",
        handle.set_launch_refused(fake(), code),
    )
    .unwrap();
}

/// The code `turn` failed with.
fn failed_with(events: &mut Events, turn: TurnId) -> ErrorCode {
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    turn_failed(events, turn)
        .unwrap_or_else(|| panic!("{}", events.dump()))
        .code
}

fn end(mut gated: Gated) {
    gated.host.settle();
    gated.events.drain_ready();
    gated.events.assert_no_mismatch();
    gated.host.shutdown();
}

#[test]
fn a_gate_answering_identity_checking_waits_and_the_launch_goes_on_once_desktop_lifts_it() {
    let checking = Arc::new(AtomicBool::new(true));
    let asked = Arc::new(AtomicUsize::new(0));
    let (flag, count) = (Arc::clone(&checking), Arc::clone(&asked));
    let mut gated = gated("launch_gate_checking", move |handle, _agent| {
        count.fetch_add(1, Ordering::SeqCst);
        if !flag.load(Ordering::SeqCst) {
            return None;
        }
        // Desktop saw the login file change: it refuses first, then answers.
        refuse(handle, Some(ErrorCode::AgentIdentityChecking));
        Some(ErrorCode::AgentIdentityChecking)
    });
    let started = gated.host.clock.now().mono;
    let (turn, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    until_keys_wait_armed(&gated.host);
    assert_eq!(
        *gated.retries.lock().unwrap(),
        vec![(fake(), ErrorCode::AgentIdentityChecking)],
        "desktop asked for the check, with the code"
    );
    assert_eq!(
        gated.launches.lock().unwrap().len(),
        0,
        "nothing launched yet"
    );
    // The check takes 300 ms and confirms the account: desktop lifts the refusal.
    gated.host.clock.advance(CHECK_TAKES);
    checking.store(false, Ordering::SeqCst);
    refuse(&gated.host.handle, None);
    assert_eq!(
        gated.events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        gated.events.dump()
    );
    // The lift woke the wait: the host clock never reached the 2 s timer.
    let waited = gated.host.clock.now().mono.saturating_sub(started);
    assert!(waited < KEYS_WAIT, "waited {waited:?}");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "the gate is asked again after the wait"
    );
    assert_eq!(gated.launches.lock().unwrap().len(), 1);
    end(gated);
}

#[test]
fn a_gate_answering_identity_checking_then_account_changed_fails_the_turn_with_it() {
    let changed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&changed);
    let mut gated = gated("launch_gate_changed", move |handle, _agent| {
        if flag.load(Ordering::SeqCst) {
            return Some(ErrorCode::SigninAccountChanged);
        }
        refuse(handle, Some(ErrorCode::AgentIdentityChecking));
        Some(ErrorCode::AgentIdentityChecking)
    });
    let (turn, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    until_keys_wait_armed(&gated.host);
    // Desktop's check finds another account.
    changed.store(true, Ordering::SeqCst);
    refuse(&gated.host.handle, Some(ErrorCode::SigninAccountChanged));
    let code = failed_with(&mut gated.events, turn);
    assert_eq!(code, ErrorCode::SigninAccountChanged);
    assert_eq!(
        gated.launches.lock().unwrap().len(),
        0,
        "no process started"
    );
    end(gated);
}

#[test]
fn a_gate_answering_a_waited_out_code_twice_fails_the_turn_after_one_wait() {
    let mut gated = gated("launch_gate_twice", move |handle, _agent| {
        refuse(handle, Some(ErrorCode::AgentIdentityChecking));
        Some(ErrorCode::AgentIdentityChecking)
    });
    let (turn, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    until_keys_wait_armed(&gated.host);
    // Desktop lifts it, but its gate answers the same again at the next look.
    refuse(&gated.host.handle, None);
    let code = failed_with(&mut gated.events, turn);
    assert_eq!(code, ErrorCode::AgentIdentityChecking);
    assert_eq!(
        gated.retries.lock().unwrap().len(),
        1,
        "one wait per launch"
    );
    assert_eq!(
        gated.launches.lock().unwrap().len(),
        0,
        "no process started"
    );
    end(gated);
}

#[test]
fn a_gate_answering_a_code_that_is_no_launch_refusal_fails_closed() {
    let mut gated = gated("launch_gate_not_refusal", |_handle, _agent| {
        Some(ErrorCode::CommandInvalid)
    });
    let (turn, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    let code = failed_with(&mut gated.events, turn);
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert!(code.is_retryable());
    assert_eq!(
        gated.launches.lock().unwrap().len(),
        0,
        "no process started"
    );
    end(gated);
}

#[test]
fn a_gate_that_panics_fails_closed_and_the_host_stays_up() {
    let panics = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&panics);
    let mut gated = gated("launch_gate_panics", move |_handle, _agent| {
        assert!(!flag.load(Ordering::SeqCst), "the gate's own bug");
        None
    });
    let (first, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    let code = failed_with(&mut gated.events, first);
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert_eq!(
        gated.launches.lock().unwrap().len(),
        0,
        "no process started"
    );
    // The host is up: with the gate fixed, the next prompt launches.
    panics.store(false, Ordering::SeqCst);
    let (second, _) = gated.host.prompt(gated.thread, "Look again.");
    assert_eq!(
        gated.events.until_turn_ended(second).stop,
        StopReason::EndTurn
    );
    assert_eq!(gated.launches.lock().unwrap().len(), 1);
    end(gated);
}

#[test]
fn a_gate_answering_none_leaves_the_launch_as_it_was() {
    let asked = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&asked);
    let mut gated = gated("launch_gate_none", move |_handle, _agent| {
        count.fetch_add(1, Ordering::SeqCst);
        None
    });
    let (turn, _) = gated.host.prompt(gated.thread, "Look at the matter.");
    assert_eq!(
        gated.events.until_turn_ended(turn).stop,
        StopReason::EndTurn
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "asked once, before the launch"
    );
    assert!(gated.retries.lock().unwrap().is_empty());
    assert_eq!(gated.launches.lock().unwrap().len(), 1);
    end(gated);
}
