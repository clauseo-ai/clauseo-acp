//! An agent the lawyer added before, found at start with no install for this build's pins (an
//! app update's new lock names a new install folder, and the old one goes): it installs again
//! with no press (`AgentsConfig::reinstall`), and its first state is the download from 0, never
//! `Missing`, so no window offers to add it. A download that cannot run fails as any install
//! does, with its Try again. An agent never added still waits for a press.

use clauseo_agents::testing::Behaviour;
use clauseo_agents::{FailureKind, InstallState};

use crate::support::{Fixture, agent_a, agent_b, tarball_path};
use crate::write;

/// The fixture with B IT's: its `install` answers `Managed` at once, which tells a test the
/// worker has handled everything before it.
fn fixture(label: &str) -> Fixture {
    let mut fx = Fixture::new(label);
    fx.config.managed.insert(agent_b());
    fx
}

/// Installs A, then renames its folder to another hash's name, as an older build's install is
/// to this build's pins.
fn installed_by_an_older_build(fx: &Fixture) -> std::path::PathBuf {
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    drop(agents);
    let root = fx.install_root(&agent_a());
    let ready = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .unwrap();
    let older = root.join("0123456789abcdef");
    write::rename(&ready, &older).unwrap();
    older
}

#[test]
fn an_added_agent_with_an_older_build_s_folder_installs_again_at_start_never_missing() {
    let mut fx = fixture("reinstall-again");
    let older = installed_by_an_older_build(&fx);
    let fetched = fx.requests_for("adapter-a").len();
    fx.config.reinstall.insert(agent_a());
    let (_agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(
        matches!(a, InstallState::Downloading { received: 0, total } if total > 0),
        "the download from 0, never Missing: {a:?}"
    );
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(
        seen.iter()
            .all(|(_, state)| !matches!(state, InstallState::Missing { .. })),
        "{seen:?}"
    );
    assert!(!older.exists(), "the older build's folder went at start");
    assert!(
        fx.requests_for("adapter-a").len() > fetched,
        "fetched again"
    );
}

#[test]
fn an_agent_never_added_with_the_same_folder_stays_missing_and_fetches_nothing() {
    let fx = fixture("reinstall-never-added");
    installed_by_an_older_build(&fx);
    let fetched = fx.requests_for("adapter-a").len();
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Missing { .. }), "{a:?}");
    agents.install(&agent_b());
    let seen = events.until(&agent_b(), |s| *s == InstallState::Managed);
    assert!(
        seen.iter().all(|(agent, _)| *agent != agent_a()),
        "{seen:?}"
    );
    assert_eq!(
        fx.requests_for("adapter-a").len(),
        fetched,
        "nothing fetched"
    );
}

#[test]
fn an_added_agent_s_install_that_a_proxy_blocks_fails_with_try_again_not_missing() {
    let mut fx = fixture("reinstall-blocked");
    installed_by_an_older_build(&fx);
    fx.config.reinstall.insert(agent_a());
    fx.registry
        .then(&tarball_path("adapter-a"), Behaviour::Status(403));
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Downloading { .. }), "{a:?}");
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("{last:?}");
    };
    assert_eq!(failure.kind, FailureKind::Blocked);
    // Try again (`RetryAgentInstall`) starts it over, and it is Ready.
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
}
