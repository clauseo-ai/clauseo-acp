//! The download rules: resume, integrity, offline, blocked, disk space and the log.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clauseo_agents::testing::Behaviour;
use clauseo_agents::{Failure, FailureKind, InstallState, TestResolver};

use crate::support::{BASE, Fixture, agent_a, node_entry, snapshot, tarball_path};

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// The failure `state` holds; panics on any other state.
fn failure(state: InstallState) -> Failure {
    match state {
        InstallState::Failed(failure) => failure,
        other => panic!("not failed: {other:?}"),
    }
}

#[test]
fn a_cut_download_resumes_by_range() {
    let fx = Fixture::new("cut");
    let cut = fx.tarballs["adapter-a"].len() / 2;
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::CutAfter(cut));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.timer.fire_next(secs(2));
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let ranges: Vec<_> = fx
        .requests_for("adapter-a")
        .into_iter()
        .map(|request| request.range)
        .collect();
    assert_eq!(ranges, [None, Some(format!("bytes={cut}-"))]);
    assert_eq!(fx.timer.install_waits(), [secs(2)]);
}

#[test]
fn the_third_tampered_tarball_is_failed_after_waits_of_2_and_8_seconds() {
    let fx = Fixture::new("tamper");
    for _ in 0..3 {
        fx.registry.then(&tarball_path("dep-a"), Behaviour::Tamper);
    }
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    let (_, last) = events.until_end(&agent_a());
    let failure = failure(last);
    assert_eq!(failure.kind, FailureKind::Failed);
    assert_eq!(fx.requests_for("dep-a").len(), 3);
    assert_eq!(fx.timer.install_waits(), [secs(2), secs(8)]);
    let log = failure.log.to_string();
    assert!(log.ends_with("result: failed after 3 tries"), "{log}");
    assert!(
        log.starts_with("Clauseo download log\nagent: agent-a"),
        "{log}"
    );
}

#[test]
fn offline_waits_30_seconds_with_no_try_counted_and_resumes_by_itself() {
    let mut fx = Fixture::new("offline");
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
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let seen = events.until(&agent_a(), |s| matches!(s, InstallState::Waiting { .. }));
    let Some((_, InstallState::Waiting { reason, log })) = seen.last() else {
        unreachable!()
    };
    assert_eq!(*reason, FailureKind::Offline);
    let log = log.to_string();
    assert!(log.contains("host: registry.test"), "{log}");
    assert!(log.ends_with("result: offline after 0 tries"), "{log}");
    online.store(true, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.timer.install_waits(), [secs(30)]);
}

#[test]
fn a_403_a_407_and_a_tls_failure_are_each_blocked_with_no_wait() {
    for case in ["403", "407", "tls"] {
        let fx = if case == "tls" {
            // https to the plain-HTTP registry: the handshake fails.
            Fixture::with_base("blocked-tls", "https://registry.test/")
        } else {
            Fixture::new("blocked")
        };
        if let Ok(code) = case.parse() {
            fx.registry
                .then(&tarball_path("adapter-a"), Behaviour::Status(code));
        }
        let (agents, events) = fx.start();
        events.first_states();
        agents.install(&agent_a());
        let (_, last) = events.until_end(&agent_a());
        let failure = failure(last);
        assert_eq!(failure.kind, FailureKind::Blocked, "{case}");
        assert_eq!(fx.timer.install_waits(), Vec::<Duration>::new(), "{case}");
        let log = failure.log.to_string();
        assert!(
            log.ends_with("result: blocked after 1 tries"),
            "{case}: {log}"
        );
        if case != "tls" {
            assert!(log.contains(&format!("status: {case}")), "{log}");
        }
    }
}

#[test]
fn low_disk_is_disk_full_before_any_request() {
    let mut fx = Fixture::new("disk");
    fx.config.snapshot = snapshot(BASE, node_entry(None), 1 << 60);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let failure = failure(last);
    assert_eq!(failure.kind, FailureKind::DiskFull);
    let (need, free) = (failure.need_mb.unwrap(), failure.free_mb.unwrap());
    assert!(need > free, "{need} MB needed, {free} MB free");
    assert_eq!(fx.registry.requests(), []);
    assert_eq!(fx.lookups.names(), Vec::<String>::new());
    assert_eq!(fx.timer.install_waits(), Vec::<Duration>::new());
}

#[test]
fn no_log_holds_a_mirror_s_user_name_or_password() {
    let mut fx = Fixture::new("secret");
    fx.config.install.registry = Some("http://rohan:hunter2@mirror.test/".to_owned());
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Status(403));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (seen, last) = events.until_end(&agent_a());
    let log = failure(last).log.to_string();
    assert!(log.contains("host: mirror.test"), "{log}");
    let everything = format!("{seen:?}");
    for secret in ["rohan", "hunter2"] {
        assert!(!everything.contains(secret), "{everything}");
    }
}
