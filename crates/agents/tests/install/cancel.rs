//! A cancelled install (`Agents::cancel_install`): the download stops within a bound, the
//! agent is `Missing` at once, and its own partial files go (its downloads, its npm cache, its
//! temp trees) while Node's archive, another agent's files and anything outside stay. A later
//! install works, also one sent at once, which supersedes the clean-up. Cancel during `npm ci`
//! is in `quit.rs`, beside its fake npm. A start keeps an agent's downloads for the next try
//! to resume.
#![expect(
    clippy::disallowed_methods,
    reason = "a cancelled job's end is real time: the tests poll its clean-up and sleep between"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_agents::InstallState;
use clauseo_agents::testing::{Behaviour, instant, millis_since};
use sha2::Digest as _;

use crate::support::{Events, Fixture, agent_a, agent_b, states_of, tarball_path};
use crate::write;

/// How long the stopped download and the clean-up after it may take. The trickled download
/// sends a byte every 200 ms, so its job sees the cancel within one.
const BOUND: Duration = Duration::from_secs(5);
/// `shutdown`'s wait.
const WAIT: Duration = Duration::from_secs(10);

/// What the tests plant: agent A's partial files, and what must stay.
struct Planted {
    a_downloads: PathBuf,
    a_cache: PathBuf,
    a_tmp: PathBuf,
    stay: Vec<PathBuf>,
}

/// Plants A's stale part file, npm cache entry and temp tree, and the files no cancel of A
/// touches: a Node archive stand-in in the shared downloads, B's download and cache, and a file
/// outside `agents/`. Run after the first states: the start's clean-up would take some.
fn plant(data: &Path) -> Planted {
    let agents = data.join("agents");
    let a_downloads = agents.join("downloads").join("agent-a");
    let a_cache = agents.join("npm-cache").join("agent-a");
    let a_tmp = agents.join("agent-a").join("install").join(".tmp-old-1");
    let stay = vec![
        agents
            .join("downloads")
            .join("node-v24.22.0-stand-in.tar.gz"),
        agents
            .join("downloads")
            .join("agent-b")
            .join("adapter-b.tgz"),
        agents.join("npm-cache").join("agent-b").join("index"),
        data.join("outside.txt"),
    ];
    for path in [&a_downloads.join("stale.tgz.part"), &a_cache.join("index")] {
        write::write(path, b"partial").unwrap();
    }
    write::write(&a_tmp.join("node_modules").join("x.js"), b"partial").unwrap();
    for path in &stay {
        write::write(path, b"keep").unwrap();
    }
    Planted {
        a_downloads,
        a_cache,
        a_tmp,
        stay,
    }
}

/// Whether A's partial files are gone, polled until [`BOUND`] from `since`.
fn gone_in_bound(planted: &Planted, since: std::time::Instant) -> bool {
    loop {
        let gone = [&planted.a_downloads, &planted.a_cache, &planted.a_tmp]
            .iter()
            .all(|path| !path.exists());
        if gone {
            return true;
        }
        if millis_since(since) > BOUND.as_millis() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A fixture whose adapter-a download trickles, started, A's install going.
fn trickling(label: &str) -> (Fixture, clauseo_agents::Agents, Events, InstallState) {
    let fx = Fixture::new(label);
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Trickle);
    let (agents, events) = fx.start();
    let (first, _) = events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    (fx, agents, events, first)
}

#[test]
fn a_cancelled_download_stops_in_bound_and_removes_only_its_own_partial_files() {
    let (fx, agents, events, first) = trickling("cancel-download");
    let planted = plant(fx.data.path());
    let since = instant();
    agents.cancel_install(&agent_a());
    let seen = events.until(&agent_a(), |s| matches!(s, InstallState::Missing { .. }));
    assert_eq!(
        seen.last().map(|(_, s)| s),
        Some(&first),
        "the first size again"
    );
    assert!(
        !states_of(&seen, &agent_a()).contains(&InstallState::Paused),
        "{seen:?}"
    );
    // The clean-up runs only once the stopped job's thread has ended (the agent's turn lock).
    assert!(
        gone_in_bound(&planted, since),
        "partial files still there after {BOUND:?}"
    );
    for path in &planted.stay {
        assert!(path.exists(), "{} was removed", path.display());
    }
    assert_eq!(agents.state(&agent_a()), Some(first));
    // A later install starts afresh and works.
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(agents.shutdown(WAIT), "a job still runs");
}

#[test]
fn an_install_sent_at_once_after_a_cancel_supersedes_the_clean_up() {
    let (_fx, agents, events, _first) = trickling("cancel-reinstall");
    agents.cancel_install(&agent_a());
    agents.install(&agent_a());
    let seen = events.until(&agent_a(), |s| matches!(s, InstallState::Missing { .. }));
    assert!(!states_of(&seen, &agent_a()).contains(&InstallState::Paused));
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let states = states_of(&seen, &agent_a());
    assert!(
        !states.contains(&InstallState::Paused),
        "the stopped job and the clean-up say nothing: {states:?}"
    );
    assert!(agents.shutdown(WAIT), "a job still runs");
}

/// A start keeps an agent's `downloads/<id>/`, unlike its npm cache: a try a quit cut short
/// left a `.part` there, and the next try resumes it with `Range`. So a cancel's clean-up a
/// busy lock skipped leaves its tarballs until the agent's next `Ready`, which removes them.
#[test]
fn a_start_keeps_a_partial_download_and_the_next_install_resumes_it() {
    let fx = Fixture::new("cancel-restart");
    let tarball = &fx.tarballs["adapter-a"];
    let cut = tarball.len() / 2;
    let hex = crate::support::to_hex(&sha2::Sha512::digest(tarball));
    let downloads = fx
        .data
        .path()
        .join("agents")
        .join("downloads")
        .join("agent-a");
    let part = downloads.join(format!("{hex}.tgz.part"));
    write::write(&part, &tarball[..cut]).unwrap();
    let (agents, events) = fx.start();
    events.first_states();
    assert!(part.exists(), "the start removed the partial download");
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let ranges: Vec<_> = fx
        .requests_for("adapter-a")
        .into_iter()
        .map(|request| request.range)
        .collect();
    assert_eq!(ranges, [Some(format!("bytes={cut}-"))]);
    assert!(!downloads.exists(), "Ready removes the agent's downloads");
    assert!(agents.shutdown(WAIT));
}

#[test]
fn a_cancel_of_a_missing_or_managed_agent_does_nothing() {
    let mut fx = Fixture::new("cancel-nothing");
    // B is IT's: its `install` answers `Managed` at once, so the test knows the worker has
    // handled every command sent before it.
    fx.config.managed.insert(agent_b());
    let (agents, events) = fx.start();
    let (first, _) = events.first_states();
    agents.cancel_install(&agent_a());
    agents.cancel_install(&agent_b());
    agents.install(&agent_b());
    let seen = events.until(&agent_b(), |s| *s == InstallState::Managed);
    assert_eq!(states_of(&seen, &agent_a()), [], "{seen:?}");
    assert_eq!(states_of(&seen, &agent_b()), [InstallState::Managed]);
    assert_eq!(agents.state(&agent_a()), Some(first));
    assert!(agents.shutdown(WAIT));
}
