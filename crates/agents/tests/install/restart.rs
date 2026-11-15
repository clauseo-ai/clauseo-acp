//! An agent's install cut short by a quit, found at the next start: its marker
//! (`agents/<id>/.install-wanted`, `install::restart`) resumes it with no press, reusing the
//! part of a tarball the quit left. An install never pressed, or paused or cancelled by the
//! lawyer, stays where it was.

use std::time::Duration;

use clauseo_agents::InstallState;
use clauseo_agents::testing::Behaviour;

use crate::support::{Fixture, agent_a, agent_b, tarball_path, to_hex};
use crate::write;

fn is_ready(state: &InstallState) -> bool {
    matches!(state, InstallState::Ready { .. })
}

fn marker(fx: &Fixture) -> std::path::PathBuf {
    fx.data.path().join("agents/agent-a/.install-wanted")
}

fn ranges(fx: &Fixture) -> Vec<Option<String>> {
    fx.requests_for("adapter-a")
        .into_iter()
        .map(|r| r.range)
        .collect()
}

/// The fixture with B IT's: its `install` answers `Managed` at once, which tells a test the
/// worker has handled everything before it.
fn fixture(label: &str) -> Fixture {
    let mut fx = Fixture::new(label);
    fx.config.managed.insert(agent_b());
    fx
}

#[test]
fn closed_mid_download_the_install_resumes_its_part_at_the_next_start_with_no_press() {
    use sha2::Digest as _;
    let fx = fixture("restart-part");
    let bytes = fx.tarballs["adapter-a"].clone();
    let half = bytes.len() / 2;
    let name = format!("{}.tgz.part", to_hex(&sha2::Sha512::digest(&bytes)));
    let downloads = fx.data.path().join("agents/downloads/agent-a");
    write::write(&downloads.join(name), &bytes[..half]).unwrap();
    write::write(&marker(&fx), b"1\n").unwrap();
    let (_agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    assert_eq!(ranges(&fx), [Some(format!("bytes={half}-"))]);
    assert!(!marker(&fx).exists(), "the marker goes at Ready");
}

#[test]
fn a_real_quit_mid_download_is_resumed_at_the_next_start() {
    let fx = fixture("restart-quit");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    assert!(marker(&fx).exists(), "written as the job starts");
    // The quit: every job cancelled; the stalled read ends once the rest is sent.
    let _: bool = agents.shutdown(Duration::ZERO);
    fx.registry.release();
    assert!(agents.shutdown(Duration::from_secs(20)), "the job ended");
    drop(agents);
    assert!(marker(&fx).exists(), "a quit keeps it");
    let (_agents, events) = fx.start();
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    let ranges = ranges(&fx);
    // The cut-short file is kept whole (no second request) or resumed, never fetched again.
    assert!(ranges.len() <= 2, "{ranges:?}");
    assert_eq!(ranges.first(), Some(&None), "{ranges:?}");
    assert!(!marker(&fx).exists());
}

#[test]
fn an_install_never_pressed_or_paused_by_the_lawyer_stays_put_at_the_next_start() {
    let fx = fixture("restart-paused");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    agents.pause(&agent_a());
    events.until(&agent_a(), |s| *s == InstallState::Paused);
    fx.registry.release();
    assert!(agents.shutdown(Duration::from_secs(20)), "the job ended");
    drop(agents);
    assert!(!marker(&fx).exists(), "a pause removes it");
    let before = ranges(&fx).len();
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
    agents.install(&agent_b());
    let seen = events.until(&agent_b(), |s| *s == InstallState::Managed);
    assert!(
        seen.iter().all(|(agent, _)| *agent != agent_a()),
        "{seen:?}"
    );
    assert_eq!(ranges(&fx).len(), before, "nothing fetched");
    assert_eq!(agents.state(&agent_a()), Some(a));
}

#[test]
fn an_install_the_lawyer_cancelled_is_not_resumed_at_the_next_start() {
    let fx = fixture("restart-cancelled");
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    assert!(marker(&fx).exists(), "written as the job starts");
    agents.cancel_install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Missing { .. }));
    fx.registry.release();
    assert!(agents.shutdown(Duration::from_secs(20)), "the jobs ended");
    drop(agents);
    assert!(!marker(&fx).exists(), "a cancel removes it");
    let before = ranges(&fx).len();
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
    agents.install(&agent_b());
    let seen = events.until(&agent_b(), |s| *s == InstallState::Managed);
    assert!(
        seen.iter().all(|(agent, _)| *agent != agent_a()),
        "{seen:?}"
    );
    assert_eq!(ranges(&fx).len(), before, "nothing fetched");
}
