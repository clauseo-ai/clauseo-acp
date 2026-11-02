//! One download at a time: a second `install` starts nothing, and a switch pauses.

use clauseo_agents::InstallState;
use clauseo_agents::testing::Behaviour;

use crate::support::{Fixture, agent_a, agent_b, states_of, tarball_path};

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

#[test]
fn a_switch_pauses_and_the_paused_install_resumes_by_range() {
    let fx = Fixture::new("switch");
    let path = tarball_path("adapter-a");
    fx.registry.then(&path, Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    fx.registry.wait_for_stalls(1);
    // The first half is written once A reports bytes: adapter-a is its first file.
    events.until(
        &agent_a(),
        |s| matches!(s, InstallState::Downloading { received, .. } if *received > 0),
    );
    agents.install(&agent_b());
    events.until(&agent_a(), |s| *s == InstallState::Paused);
    assert_eq!(agents.state(&agent_a()), Some(InstallState::Paused));
    fx.registry.release();
    let (seen, last) = events.until_end(&agent_b());
    assert!(is_ready(&last), "{last:?}");
    // The paused job says nothing after its `Paused`.
    assert_eq!(states_of(&seen, &agent_a()), []);
    assert_eq!(states_of(&events.drain(), &agent_a()), []);

    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(is_ready(&last), "{last:?}");
    let half = fx.tarballs["adapter-a"].len() / 2;
    let ranges: Vec<_> = fx
        .requests_for("adapter-a")
        .into_iter()
        .map(|request| request.range)
        .collect();
    assert_eq!(ranges, [None, Some(format!("bytes={half}-"))]);
}
