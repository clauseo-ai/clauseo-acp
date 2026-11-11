//! Two installs at once (decision 0033, "Two installs at once"): a second `install` of the same
//! agent starts nothing; another agent's install runs beside it, never pausing it; a pause stops
//! only its own agent; the free space is counted for both; and another process holding
//! `agents/.lock` (an older Clauseo) stops both the start-up clean-up and an install, while one
//! holding only an agent's `.lock-<id>` stops that agent's alone.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_agents::testing::Behaviour;
use clauseo_agents::{FailureKind, InstallState};

use crate::support::{Fixture, agent_a, agent_b, platform, states_of, tarball_path};
use crate::write;

fn is_ready(state: &InstallState) -> bool {
    matches!(state, InstallState::Ready { .. })
}

#[test]
fn a_second_install_starts_nothing() {
    let mut fx = Fixture::new("second");
    // B is IT's: its `install` answers `Managed` at once, which tells the test that the
    // worker has handled every command sent before it.
    fx.config.managed.insert(agent_b());
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    let (_, b) = events.first_states();
    assert_eq!(b, InstallState::Managed);
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    agents.install(&agent_a());
    agents.install(&agent_b());
    let mut seen = events.until(&agent_b(), |s| *s == InstallState::Managed);
    fx.registry.release();
    let (rest, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    seen.extend(rest);
    let states = states_of(&seen, &agent_a());
    assert_eq!(
        states.iter().filter(|s| is_ready(s)).count(),
        1,
        "{states:?}"
    );
    assert!(!states.contains(&InstallState::Paused), "{states:?}");
    for name in ["adapter-a", "dep-a", "peer-a"] {
        assert_eq!(fx.requests_for(name).len(), 1, "{name}");
    }
}

/// Starting B while A downloads pauses nothing: B runs to `Ready` while A's download is held
/// mid-file, A never says `Paused`, and A's file is fetched once, with no resumed range.
#[test]
fn starting_b_while_a_downloads_pauses_nothing_and_both_reach_ready() {
    let fx = Fixture::new("beside");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    events.until(
        &agent_a(),
        |s| matches!(s, InstallState::Downloading { received, .. } if *received > 0),
    );
    agents.install(&agent_b());
    // B finishes while A's download is still held: the two ran at once.
    let (seen, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
    assert!(!states_of(&seen, &agent_a()).contains(&InstallState::Paused));
    assert!(
        matches!(
            agents.state(&agent_a()),
            Some(InstallState::Downloading { .. })
        ),
        "{:?}",
        agents.state(&agent_a())
    );
    fx.registry.release();
    let (seen, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    assert!(!states_of(&seen, &agent_a()).contains(&InstallState::Paused));
    let ranges: Vec<_> = fx
        .requests_for("adapter-a")
        .into_iter()
        .map(|request| request.range)
        .collect();
    assert_eq!(ranges, [None], "fetched once, never paused and resumed");
}

/// Two installs started together both reach `Ready`, their progress interleaved: both
/// downloads are held mid-file at once, so each agent sends progress before the other ends.
#[test]
fn two_installs_started_together_both_reach_ready_interleaving_their_progress() {
    let fx = Fixture::new("together");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    fx.registry
        .then(&tarball_path("adapter-b"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    agents.install(&agent_b());
    // Both downloads are in flight together.
    fx.registry.wait_for_stalls(2);
    fx.registry.release();
    let (mut seen, last_a) = events.until_end(&agent_a());
    if !matches!(agents.state(&agent_b()), Some(InstallState::Ready { .. })) {
        let (rest, last_b) = events.until_end(&agent_b());
        assert!(is_ready(&last_b), "{last_b:?}");
        seen.extend(rest);
    }
    assert!(is_ready(&last_a), "{last_a:?}");
    assert!(agents.installed(&agent_a()).is_some() && agents.installed(&agent_b()).is_some());
    // Each agent's progress began before the other's install ended.
    let first_of = |agent| {
        seen.iter()
            .position(|(id, s)| *id == agent && matches!(s, InstallState::Downloading { .. }))
    };
    let ready_of = |agent| seen.iter().position(|(id, s)| *id == agent && is_ready(s));
    let (a_first, b_first) = (first_of(agent_a()), first_of(agent_b()));
    let (a_ready, b_ready) = (ready_of(agent_a()), ready_of(agent_b()));
    assert!(
        a_first.is_some() && a_first < b_ready.or(Some(usize::MAX)),
        "{seen:?}"
    );
    assert!(b_first.is_some() && b_first < a_ready, "{seen:?}");
    for (id, state) in &seen {
        assert_ne!(*state, InstallState::Paused, "{id}");
    }
}

/// Pausing A while both download leaves B running: A says `Paused` and nothing after it, and
/// B reaches `Ready`.
#[test]
fn cancelling_a_leaves_b_running() {
    let fx = Fixture::new("cancel-one");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    fx.registry
        .then(&tarball_path("adapter-b"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    agents.install(&agent_b());
    fx.registry.wait_for_stalls(2);
    agents.pause(&agent_a());
    let seen = events.until(&agent_a(), |s| *s == InstallState::Paused);
    assert!(!states_of(&seen, &agent_b()).contains(&InstallState::Paused));
    fx.registry.release();
    let (seen, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
    assert_eq!(
        states_of(&seen, &agent_a()),
        [],
        "the paused job says nothing more"
    );
    assert_eq!(agents.state(&agent_a()), Some(InstallState::Paused));
}

/// The free space is counted for both: with room for A alone, A starts and B, started while A
/// is in flight, fails `DiskFull` with the need of both, while A goes on to `Ready`. Once A
/// is done its need is no longer counted, and B's retry installs.
#[test]
fn the_disk_check_counts_the_install_in_flight() {
    let mut fx = Fixture::new("disk-both");
    let need = |agent| {
        fx.config
            .snapshot
            .need_bytes(&agent, platform(), false, false)
            .unwrap()
    };
    let (need_a, need_b) = (need(agent_a()), need(agent_b()));
    // Room for A, and for B alone, never for B beside A's remaining need.
    let free = need_a + need_b / 2;
    fx.config.install.free_bytes = Some(Arc::new(move || Some(free)));
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    agents.install(&agent_b());
    let (seen, last) = events.until_end(&agent_b());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert_eq!(failure.kind, FailureKind::DiskFull, "{}", failure.log);
    assert_eq!(failure.kind.code(), "agent.install_disk_full");
    assert_eq!(failure.free_mb, Some(free / 1_000_000));
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line.ends_with("MB needed")),
        "{}",
        failure.log
    );
    assert!(!states_of(&seen, &agent_a()).contains(&InstallState::Paused));
    assert_eq!(fx.requests_for("adapter-b"), [], "B fetched nothing");
    fx.registry.release();
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");

    agents.retry(&agent_b(), false);
    let (_, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
}

/// What an install in flight has written counts once (decision 0033, "The disk need"): with
/// about 1.5 GB free and each agent needing about 700 MB, A has written 600 MB of its need (a
/// sparse file in its npm cache stands in for its `npm ci`), the free space has dropped by as
/// much, and B, started then, fits: A's remaining 100 MB and B's 700 MB are under the 900 MB
/// left. Counted twice (A's whole need beside the lower free space) B would be `DiskFull`.
#[cfg(unix)]
#[test]
fn what_an_install_in_flight_has_written_counts_once() {
    const FREE: u64 = 1_500_000_000;
    const WRITTEN: u64 = 600_000_000;
    let mut fx = Fixture::new("disk-once");
    fx.config.snapshot = crate::support::snapshot(
        crate::support::BASE,
        crate::support::node_entry(None),
        636_000_000,
    );
    let need = |agent| {
        fx.config
            .snapshot
            .need_bytes(&agent, platform(), false, false)
            .unwrap()
    };
    let (need_a, need_b) = (need(agent_a()), need(agent_b()));
    assert!((690_000_000..710_000_000).contains(&need_a), "{need_a}");
    assert!(
        need_a + need_b > FREE - WRITTEN,
        "counted twice, B would not fit"
    );
    let big = fx
        .data
        .path()
        .join("agents/npm-cache/agent-a/_cacache/content-v2/big");
    let measured = big.clone();
    fx.config.install.free_bytes = Some(Arc::new(move || {
        let written = std::fs::symlink_metadata(&measured).map_or(0, |meta| meta.len());
        Some(FREE - written)
    }));
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    write::sparse(&big, WRITTEN).unwrap();
    agents.install(&agent_b());
    let (_, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
    fx.registry.release();
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    assert!(!big.exists(), "A's npm cache goes at its Ready");
}

/// An older Clauseo holds `agents/.lock` exclusive while it installs: neither the clean-up nor
/// an install runs here until it is done.
#[test]
fn another_process_holding_the_lock_skips_the_clean_up_and_fails_the_install() {
    let fx = Fixture::new("locked");
    let hash = fx
        .config
        .snapshot
        .install_hash(&agent_a(), fx.config.platform)
        .unwrap();
    let tmp = fx.install_root(&agent_a()).join(format!(".tmp-{hash}-1"));
    write::write(&tmp.join("package.json"), "{}").unwrap();
    let held = write::hold_lock(&fx.data.path().join("agents/.lock")).unwrap();

    let (agents, events) = fx.start();
    events.first_states();
    assert!(tmp.exists(), "another Clauseo's temp folder is left alone");
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line.contains("another Clauseo is installing")),
        "{}",
        failure.log
    );
    assert_eq!(fx.registry.requests(), []);

    drop(held);
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
}

/// Another Clauseo installing A holds `agents/.lock` shared and `.lock-agent-a`: A's clean-up
/// and install wait for it (the install fails at once, as before), while B's clean-up runs and
/// B installs beside it.
#[test]
fn another_process_installing_one_agent_leaves_the_other_free() {
    let fx = Fixture::new("locked-one");
    let tmp_of = |agent| {
        let hash = fx
            .config
            .snapshot
            .install_hash(&agent, fx.config.platform)
            .unwrap();
        fx.install_root(&agent).join(format!(".tmp-{hash}-1"))
    };
    let (tmp_a, tmp_b) = (tmp_of(agent_a()), tmp_of(agent_b()));
    for tmp in [&tmp_a, &tmp_b] {
        write::write(&tmp.join("package.json"), "{}").unwrap();
    }
    let agents_dir = fx.data.path().join("agents");
    let all = write::hold_shared(&agents_dir.join(".lock")).unwrap();
    let own = write::hold_lock(&agents_dir.join(".lock-agent-a")).unwrap();

    let (agents, events) = fx.start();
    events.first_states();
    assert!(
        tmp_a.exists(),
        "the other Clauseo's temp folder for A is left alone"
    );
    assert!(!tmp_b.exists(), "B's stale temp folder is cleaned up");
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line.contains("another Clauseo is installing agent-a")),
        "{}",
        failure.log
    );
    agents.install(&agent_b());
    let (_, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
    assert_eq!(fx.requests_for("adapter-a"), []);

    drop((own, all));
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
}

#[test]
fn a_relative_bundle_path_fails_the_install_clearly() {
    let mut fx = Fixture::new("relative");
    fx.config.install.bundle = Some("bundle".into());
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line == "the bundle setting is not an absolute path"),
        "{}",
        failure.log
    );
}

/// A job whose thread panics (a dev or test build; a release build aborts) frees its slot: A
/// ends `Failed` saying so, B installs beside it, and A's retry installs.
#[test]
fn a_panicking_install_job_frees_its_slot() {
    let mut fx = Fixture::new("panic");
    let armed = Arc::new(AtomicBool::new(true));
    let fire = Arc::clone(&armed);
    fx.config.install.rename_fault = Some(Arc::new(move || {
        let in_a = std::thread::current().name() == Some("clauseo-install-agent-a");
        assert!(
            !(in_a && fire.swap(false, Ordering::SeqCst)),
            "a test's panic in A's job"
        );
        None
    }));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    agents.install(&agent_b());
    let (mut seen, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert!(
        failure
            .log
            .lines()
            .iter()
            .any(|line| line == "the install stopped unexpectedly"),
        "{}",
        failure.log
    );
    if !matches!(agents.state(&agent_b()), Some(InstallState::Ready { .. })) {
        let (rest, last) = events.until_end(&agent_b());
        assert!(is_ready(&last), "{last:?}");
        seen.extend(rest);
    }
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
}
