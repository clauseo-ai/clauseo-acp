//! Resume after a relaunch: the sign-in flag (`agents/<id>/.chosen`) and the files already in
//! `agents/downloads/`. A flagged agent whose every tarball is there and checks is set up at
//! start with no fetch, and reaches `Ready` with no sign-in started; one tarball missing or
//! wrong, or no flag, and nothing starts (#664). `Missing`'s bytes are those still to fetch:
//! `Some(0)` with everything here, `None` with no size for the platform.

use std::path::{Path, PathBuf};

use clauseo_agents::{AgentEntry, InstallState, Snapshot};
use clauseo_protocol::AgentId;
use sha2::{Digest as _, Sha512};

use crate::support::{
    A_PACKAGES, B_PACKAGES, BASE, Events, Fixture, agent_a, agent_b, node_entry, pins, states_of,
    to_hex,
};
use crate::write;

/// `<data>/agents/<id>/.chosen`.
fn flag(fx: &Fixture, agent: &AgentId) -> PathBuf {
    fx.data
        .path()
        .join("agents")
        .join(agent.as_str())
        .join(".chosen")
}

/// Puts the flag there as the desktop's Sign in press does.
fn mark(fx: &Fixture, agent: &AgentId) {
    write::write(&flag(fx, agent), "sign-in pressed\n").unwrap();
}

/// `<data>/agents/downloads/<sha512-hex>.tgz` for a tarball of `bytes`.
fn download(data: &Path, bytes: &[u8]) -> PathBuf {
    data.join("agents/downloads")
        .join(format!("{}.tgz", to_hex(&Sha512::digest(bytes))))
}

/// Puts package `name`'s checked tarball in `downloads/`, as a cut-short install leaves it.
fn cache(fx: &Fixture, name: &str) -> PathBuf {
    let bytes = &fx.tarballs[name];
    let path = download(fx.data.path(), bytes);
    write::write(&path, bytes).unwrap();
    path
}

fn size(fx: &Fixture, names: &[&str]) -> u64 {
    names
        .iter()
        .map(|name| u64::try_from(fx.tarballs[name].len()).unwrap())
        .sum()
}

/// Installs B, and checks A had nothing at start: every event up to B's `Ready` is B's, and
/// only B's tarballs were requested. A job for A started at start would show (its own events,
/// or the `Paused` B's install gives it).
fn nothing_started_for_a(fx: &Fixture, agents: &clauseo_agents::Agents, events: &Events) {
    agents.install(&agent_b());
    let (seen, last) = events.until_end(&agent_b());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(states_of(&seen, &agent_a()), [], "nothing for A");
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 0, "{name} not fetched");
    }
    let a = agents.state(&agent_a());
    assert!(matches!(a, Some(InstallState::Missing { .. })), "{a:?}");
}

#[test]
fn a_flagged_agent_with_every_tarball_here_is_set_up_at_start_with_no_fetch_and_no_sign_in() {
    let fx = Fixture::new("resume-flagged");
    mark(&fx, &agent_a());
    for name in A_PACKAGES {
        cache(&fx, name);
    }

    let (agents, events) = fx.start();
    let (a, b) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(0)
        },
        "everything is here"
    );
    assert_eq!(
        b,
        InstallState::Missing {
            download_bytes: Some(size(&fx, &B_PACKAGES))
        }
    );
    // No `install` call: the start runs it. `Events::next` refuses any sign-in event.
    let (seen, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.registry.requests().len(), 0, "no fetch at all");
    assert_eq!(agents.install_tries(&agent_a()), Some(1));
    assert_eq!(agents.signin_state(&agent_a()), None, "no sign-in");
    assert!(!agents.sign_in_going(&agent_a()), "no sign-in reserved");
    assert!(agents.installed(&agent_a()).is_some());
    assert!(states_of(&seen, &agent_b()).is_empty(), "B untouched");
}

#[test]
fn a_flagged_agent_with_a_tarball_missing_or_wrong_starts_nothing() {
    // One missing.
    let fx = Fixture::new("resume-one-missing");
    mark(&fx, &agent_a());
    cache(&fx, "adapter-a");
    cache(&fx, "dep-a");
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(size(&fx, &["peer-a"]))
        },
        "the bytes still to fetch"
    );
    nothing_started_for_a(&fx, &agents, &events);
    drop(agents);

    // One that does not check: kept on disk (the start deletes nothing), nothing starts.
    let fx = Fixture::new("resume-one-wrong");
    mark(&fx, &agent_a());
    cache(&fx, "adapter-a");
    cache(&fx, "dep-a");
    let wrong = download(fx.data.path(), &fx.tarballs["peer-a"]);
    write::write(&wrong, b"not the tarball").unwrap();
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(size(&fx, &["peer-a"]))
        }
    );
    nothing_started_for_a(&fx, &agents, &events);
    assert!(wrong.is_file(), "the start deletes nothing");
}

#[test]
fn an_agent_with_no_flag_starts_nothing_with_every_tarball_here() {
    let fx = Fixture::new("resume-no-flag");
    for name in A_PACKAGES {
        cache(&fx, name);
    }
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(0)
        }
    );
    nothing_started_for_a(&fx, &agents, &events);
}

#[test]
fn mark_and_clear_write_and_remove_the_flag_with_fixed_content() {
    let fx = Fixture::new("resume-mark");
    let (agents, events) = fx.start();
    events.first_states();
    agents.mark_chosen(&agent_a());
    let content = std::fs::read(flag(&fx, &agent_a())).unwrap();
    assert_eq!(content, b"sign-in pressed\n");
    agents.clear_chosen(&agent_a());
    assert!(!flag(&fx, &agent_a()).exists());
    // Clearing none is fine.
    agents.clear_chosen(&agent_a());
}

#[test]
fn the_flag_goes_at_start_for_a_managed_agent_or_one_the_snapshot_no_longer_has() {
    let mut fx = Fixture::new("resume-managed");
    mark(&fx, &agent_a());
    for name in A_PACKAGES {
        cache(&fx, name);
    }
    let gone = fx.data.path().join("agents/gone-acp/.chosen");
    write::write(&gone, "sign-in pressed\n").unwrap();
    fx.config.managed.insert(agent_a());
    let (_agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert_eq!(a, InstallState::Managed);
    assert!(!flag(&fx, &agent_a()).exists(), "managed: the flag goes");
    assert!(!gone.exists(), "removed: the flag goes");
    assert_eq!(fx.registry.requests().len(), 0);
}

/// The fixture's snapshot with no size row for either agent.
fn no_sizes(fx: &Fixture) -> Snapshot {
    let (package_json, lock) = pins(BASE, &fx.tarballs);
    let entry = |id: AgentId, root: &str| {
        let mut entry = AgentEntry::new(id, root, root, "dist/index.js");
        entry.install_hosts = vec!["registry.test".to_owned()];
        entry
    };
    let agents = vec![entry(agent_a(), "adapter-a"), entry(agent_b(), "adapter-b")];
    Snapshot::new(&package_json, &lock, BASE, node_entry(None), agents).unwrap()
}

#[test]
fn missing_gives_the_bytes_still_to_fetch_none_when_unknown_and_0_when_all_here() {
    // Unknown: no size row.
    let mut fx = Fixture::new("size-unknown");
    fx.config.snapshot = no_sizes(&fx);
    let (agents, events) = fx.start();
    let (a, b) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: None
        }
    );
    assert_eq!(
        b,
        InstallState::Missing {
            download_bytes: None
        }
    );
    drop(agents);

    // All here: 0, with or without a row.
    for name in A_PACKAGES {
        cache(&fx, name);
    }
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(0)
        }
    );
    drop(agents);

    // Partly here: the row less what is here.
    let fx = Fixture::new("size-partly");
    cache(&fx, "adapter-a");
    let (_agents, events) = fx.start();
    let (a, b) = events.first_states();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: Some(size(&fx, &["dep-a", "peer-a"]))
        }
    );
    assert_eq!(
        b,
        InstallState::Missing {
            download_bytes: Some(size(&fx, &B_PACKAGES))
        }
    );
    assert_eq!(fx.requests_for("adapter-a").len(), 0);
}
