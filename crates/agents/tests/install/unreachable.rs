//! Offline tries in a row (#683's review): `Waiting` says offline for the first two and
//! unreachable (`agent.install_unreachable`) from the third on, while the tries go on every 30 s;
//! a try that fails another way starts the count again; while the OS reports no network at all
//! it stays offline whatever the count.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clauseo_agents::FailureKind::{self, Offline, Unreachable};
use clauseo_agents::testing::Behaviour;
use clauseo_agents::{InstallState, OsNetwork, TestResolver};

use crate::support::{Events, Fixture, agent_a, tarball_path};

const WAIT: Duration = Duration::from_secs(30);

/// The next `Waiting`'s reason; panics on an end first.
fn next_wait(events: &Events) -> FailureKind {
    let seen = events.until(&agent_a(), |s| {
        matches!(
            s,
            InstallState::Waiting { .. } | InstallState::Failed(_) | InstallState::Ready { .. }
        )
    });
    match seen.last() {
        Some((_, InstallState::Waiting { reason, log })) => {
            let word = reason.word();
            assert!(
                log.to_string().contains(&format!("result: {word} ")),
                "{log}"
            );
            *reason
        }
        other => panic!("not waiting: {other:?}"),
    }
}

/// An OS that reports no network while `down` holds.
struct FakeOs(Arc<AtomicBool>);

impl OsNetwork for FakeOs {
    fn disconnected(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A resolver that fails every lookup while `down` holds, else answers with `addr`.
fn dns(fx: &Fixture, down: &Arc<AtomicBool>) -> TestResolver {
    let addr = fx.registry.addr();
    let down = Arc::clone(down);
    TestResolver(Arc::new(move |name: &str| {
        if down.load(Ordering::SeqCst) {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no {name}"),
            ))
        } else {
            Ok(vec![addr])
        }
    }))
}

/// Every request reset (a proxy dropping every connection): offline, offline, then unreachable
/// on the third and fourth tries, the tries going on every 30 s; once the connections go through,
/// the next try finishes.
#[test]
fn three_resets_in_a_row_wait_as_unreachable_and_the_tries_go_on() {
    let fx = Fixture::new("unreachable-resets");
    let path = tarball_path("adapter-a");
    for _ in 0..4 {
        fx.registry.then(&path, Behaviour::Reset);
    }
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let mut reasons = vec![next_wait(&events)];
    for _ in 0..3 {
        fx.timer.fire_next(WAIT);
        reasons.push(next_wait(&events));
    }
    assert_eq!(reasons, [Offline, Offline, Unreachable, Unreachable]);
    assert_eq!(FailureKind::Unreachable.code(), "agent.install_unreachable");
    fx.timer.fire_next(WAIT);
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.requests_for("adapter-a").len(), 5);
    assert_eq!(fx.timer.install_waits(), [WAIT; 4]);
}

/// Name lookups failing (clearly offline) wait as offline first and as unreachable from the third
/// try, while the OS reports a network; while it reports none, every wait stays offline.
#[test]
fn dns_down_turns_unreachable_after_three_unless_the_os_reports_no_network() {
    for os_down in [false, true] {
        let mut fx = Fixture::new("unreachable-dns");
        let down = Arc::new(AtomicBool::new(true));
        fx.config.install.resolver = Some(dns(&fx, &down));
        fx.config.os_network = Arc::new(FakeOs(Arc::new(AtomicBool::new(os_down))));
        let (agents, events) = fx.start();
        events.first_states();
        agents.install(&agent_a());
        let mut reasons = vec![next_wait(&events)];
        for _ in 0..2 {
            fx.timer.fire_next(WAIT);
            reasons.push(next_wait(&events));
        }
        let third = if os_down { Offline } else { Unreachable };
        assert_eq!(reasons, [Offline, Offline, third], "OS down: {os_down}");
        down.store(false, Ordering::SeqCst);
        fx.timer.fire_next(WAIT);
        let (_, last) = events.until_end(&agent_a());
        assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    }
}

/// A try that fails another way (a tampered file, counted with its 2 s wait) starts the count
/// again: two resets, the tamper, then two more resets still wait as offline, and only the
/// third after the tamper as unreachable. A new job (after `Ready`, or a retry) starts at 0 with
/// its own `Run`.
#[test]
fn a_try_failing_another_way_starts_the_count_again() {
    let fx = Fixture::new("unreachable-reset-count");
    let path = tarball_path("adapter-a");
    fx.registry.then(&path, Behaviour::Reset);
    fx.registry.then(&path, Behaviour::Reset);
    fx.registry.then(&path, Behaviour::Tamper);
    for _ in 0..3 {
        fx.registry.then(&path, Behaviour::Reset);
    }
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let mut reasons = vec![next_wait(&events)];
    fx.timer.fire_next(WAIT);
    reasons.push(next_wait(&events));
    fx.timer.fire_next(WAIT);
    fx.timer.fire_next(Duration::from_secs(2));
    reasons.push(next_wait(&events));
    for _ in 0..2 {
        fx.timer.fire_next(WAIT);
        reasons.push(next_wait(&events));
    }
    assert_eq!(reasons, [Offline, Offline, Offline, Offline, Unreachable]);
    fx.timer.fire_next(WAIT);
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
}
