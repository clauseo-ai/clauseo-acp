//! Whether the host fetches the list at all ([`needed`]): the plain build, with the guarded
//! browser's entry proxy-free (`servers::browser_mcp`) and no sandbox wired, starts no updater
//! and makes no request; a sandbox wired, or the seams' test switch, starts it and it fetches.

use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_sandbox::{Detection, Tier};
use clauseo_settings::{Effective, Locations, SystemFiles};

use super::super::{BlockListSeams, needed, prepare, start};
use super::{NEWER, Rig, pair, put};
use crate::HostConfig;
use crate::config::Sandbox;

/// A host config under the rig's folder on its clock, with `seams` and `sandbox` when given.
fn config(rig: &Rig, seams: BlockListSeams, sandbox: Option<Sandbox>) -> HostConfig {
    let dir = rig.dir.path();
    crate::write::create_dir_all(&rig.data_dir()).unwrap();
    let effective = Effective::load(&Locations::under_home(dir), &SystemFiles, &[]);
    let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.join("home")));
    let config =
        HostConfig::new(effective, Arc::new(launcher)).with_clock(Arc::new(rig.clock.clone()));
    let config = seams.apply(config);
    match sandbox {
        Some(sandbox) => config.with_sandbox(sandbox),
        None => config,
    }
}

/// The rig's seams: its shared list, its bundled folder and its server.
fn seams(rig: &Rig) -> BlockListSeams {
    BlockListSeams::new(Arc::clone(&rig.current), Some(rig.bundled()), rig.update())
}

/// A sandbox whose detection says tier A.
fn sandbox() -> Sandbox {
    let protected = || Detection {
        tier: Tier::A,
        reason: None,
    };
    Sandbox::new(Arc::new(protected), None)
}

/// Starts the updater as the host thread does and waits for the newer list to be swapped in.
fn starts_and_fetches(rig: &Rig, config: &HostConfig) {
    assert!(needed(config));
    let running = prepare(config).and_then(start).expect("an updater");
    // Up to 30 s, polled on a timer (no sleep, no `Instant`: `clippy.toml`).
    let swapped = (0..3000).any(|_| {
        futures::executor::block_on(async_io::Timer::after(Duration::from_millis(10)));
        rig.in_use().as_deref() == Some(NEWER)
    });
    assert!(swapped, "no fetch: {:?}", rig.server.hits());
    assert!(rig.server.hits().iter().any(|hit| hit == "GET /list.gz"));
    drop(running);
}

/// The plain build: no thread, so no start-up load and no request, now or at any interval; the
/// first load's teller is gone, so a launch's wait for it would end at once. (This fails once
/// a shipped entry has a proxy again, which is when the list is fetched for every build.)
#[test]
fn the_plain_build_starts_no_updater_and_makes_no_request() {
    let rig = Rig::new("needed-plain");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let config = config(&rig, seams(&rig), None);
    assert!(!needed(&config));
    assert!(prepare(&config).and_then(start).is_none());
    let seams = config.wiring.blocklist.as_deref().unwrap();
    assert!(seams.first_load.teller().is_none());
    assert_eq!(rig.server.hits(), Vec::<String>::new());
    assert!(rig.current.get().is_unloaded());
}

#[test]
fn with_a_sandbox_wired_the_updater_starts_and_fetches() {
    let rig = Rig::new("needed-sandbox");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let config = config(&rig, seams(&rig), Some(sandbox()));
    starts_and_fetches(&rig, &config);
}

#[test]
fn seams_marked_enforced_start_the_updater_without_a_sandbox() {
    let rig = Rig::new("needed-enforced");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let config = config(&rig, seams(&rig).enforced(), None);
    starts_and_fetches(&rig, &config);
}
