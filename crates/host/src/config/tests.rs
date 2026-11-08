use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{AgentId, Timestamp};
use clauseo_settings::{Effective, Files, Locations};

use super::{ConfigParts, HostConfig, NoProfile, ProfileHook};
use crate::clock::ManualClock;
use crate::ids::SequentialIds;
use crate::window::PATCH_WINDOW_BYTES;

/// No settings file anywhere.
struct NoFiles;

impl Files for NoFiles {
    fn read(&self, _path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    fn write_atomic(&self, _path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
}

fn effective() -> Effective {
    Effective::load(&Locations::under_root(Path::new("/nowhere")), &NoFiles, &[])
}

#[test]
fn defaults_and_setters() {
    let launcher = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let parts = ConfigParts::from(HostConfig::new(effective(), launcher));
    assert_eq!(parts.patch_window_bytes, PATCH_WINDOW_BYTES);
    assert_eq!(parts.refused_folders, Vec::<PathBuf>::new());
    assert_eq!(parts.effective.data_dir(), Path::new("/nowhere"));
    let agent: AgentId = "fake-agent".parse().unwrap();
    assert!(parts.profile.session_meta(&agent, None).is_none());
    assert_eq!(
        NoProfile.launch_env(&agent),
        std::collections::BTreeMap::new()
    );

    let clock = ManualClock::new(Timestamp::new(7).unwrap());
    let launcher = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let parts: ConfigParts = HostConfig::new(effective(), launcher)
        .with_clock(Arc::new(clock.clone()))
        .with_ids(Box::new(SequentialIds::new()))
        .with_refused_folders(vec![PathBuf::from("/refused")])
        .with_profile_hook(Box::new(NoProfile))
        .with_patch_window_bytes(1024)
        .into();
    assert_eq!(parts.patch_window_bytes, 1024);
    assert_eq!(parts.refused_folders, vec![PathBuf::from("/refused")]);
    clock.advance(Duration::from_millis(3));
    assert_eq!(parts.clock.now().wall, Timestamp::new(10).unwrap());
}

#[test]
fn config_moves_to_another_thread() {
    fn is_send<T: Send>() {}
    is_send::<HostConfig>();
}

/// Stage C's switch: only `InJail` is built; `OutsideJail` is refused with a clear error for now.
#[test]
fn the_browser_outside_the_jail_is_refused_for_now() {
    use crate::browser::{BrowserLaunch, UnsupportedLaunch};
    let launcher = || Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    assert_eq!(BrowserLaunch::default(), BrowserLaunch::InJail);
    assert!(
        HostConfig::new(effective(), launcher())
            .with_browser_launch(BrowserLaunch::InJail)
            .is_ok()
    );
    let Err(refused) = HostConfig::new(effective(), launcher())
        .with_browser_launch(BrowserLaunch::OutsideJail { cdp_port: 9222 })
    else {
        panic!("OutsideJail was accepted");
    };
    assert_eq!(refused, UnsupportedLaunch { cdp_port: 9222 });
    let message = refused.to_string();
    assert!(
        message.contains("OutsideJail") && message.contains("not built"),
        "{message}"
    );
}

/// The machine's memory sets the slots: 2 by default (unknown memory) and at 8 GB, 4 above.
#[test]
fn machine_memory_sets_the_browser_slots() {
    use crate::browser::limits::BrowserSlots;
    const GB: u64 = 1024 * 1024 * 1024;
    let launcher = || Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let parts = ConfigParts::from(HostConfig::new(effective(), launcher()));
    assert_eq!(parts.wiring.browser_slots, BrowserSlots::new(Some(8 * GB)));
    let parts: ConfigParts = HostConfig::new(effective(), launcher())
        .with_machine_memory(16 * GB)
        .into();
    assert_eq!(parts.wiring.browser_slots, BrowserSlots::new(Some(32 * GB)));
}

/// A reservation is refused for another matter or launch, and released (its guard dropped) when
/// it drops; its `Debug` and an agent listener's never show the proxy's token.
#[test]
fn a_reservation_checks_its_launch_and_releases_on_drop() {
    use clauseo_protocol::{MatterId, ThreadId};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let id = crate::pipeline::tests::support::id::<MatterId>;
    let key = |matter: MatterId, launch| super::SessionKey {
        matter,
        thread: crate::pipeline::tests::support::id::<ThreadId>(9),
        launch,
    };
    let released = Arc::new(AtomicUsize::new(0));
    let reservation = super::sandbox::Reservation::fixed(vec![46_100], id(1), key(id(1), 1))
        .with_guard(Box::new(Counted(Arc::clone(&released))));
    assert_eq!(reservation.ports(), [46_100]);
    assert_eq!(
        (reservation.slot(), reservation.pool_user().is_none()),
        (None, true)
    );
    assert!(reservation.check(&id(1), &key(id(1), 1)).is_ok());
    assert!(reservation.check(&id(2), &key(id(1), 1)).is_err());
    assert!(reservation.check(&id(1), &key(id(1), 2)).is_err());
    assert_eq!(released.load(Ordering::SeqCst), 0);
    drop(reservation);
    assert_eq!(released.load(Ordering::SeqCst), 1);

    let listener = super::AgentListener::new(
        46_101,
        vec![(
            "HTTPS_PROXY".into(),
            Some("http://clauseo:0123secret@127.0.0.1:46101".into()),
        )],
    );
    let shown = format!("{listener:?}");
    assert!(
        shown.contains("HTTPS_PROXY") && !shown.contains("0123secret"),
        "{shown}"
    );
}
