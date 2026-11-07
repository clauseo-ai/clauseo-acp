//! The Scratch matter's folder scan off the actor, held by a stand-in the test releases: ready
//! goes out once [`READY_WAIT`] passes on the manual clock, and a late Scratch reaches an app
//! subscriber as any new matter does; a host stopped mid-scan makes none. Never a sleep.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{HostEvent, HostQuery, MatterKind, Principal, QueryResult, Timestamp};
use clauseo_settings::{Effective, Locations, SystemFiles};
use clauseo_store::{Store, StoreOptions};
use futures::StreamExt;
use futures::executor::block_on;

use super::{READY_WAIT, Scan, free_folder};
use crate::pipeline::tests::support::TempDir;
use crate::{AppSubscription, Host, HostConfig, HostHandle, HostJoin, ManualClock};

/// How long a test waits for the host's threads before it fails.
const PATIENCE: Duration = Duration::from_secs(30);

/// A scan stand-in: it says when it is entered, waits for the test's release, then scans as the
/// host does and says when it has answered.
struct Held {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    answered: mpsc::Receiver<Option<PathBuf>>,
}

fn held_scan() -> (Scan, Held) {
    let (enter, entered) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (answer, answered) = mpsc::channel();
    let released = Mutex::new(released);
    let scan: Scan = Arc::new(move |base: &Path| {
        let _told = enter.send(());
        let _released = released.lock().map(|released| released.recv());
        let found = free_folder(base);
        let _told = answer.send(found.clone());
        found
    });
    let held = Held {
        entered,
        release,
        answered,
    };
    (scan, held)
}

fn config(dir: &Path, clock: &ManualClock, run: u16, scan: Option<Scan>) -> HostConfig {
    crate::write::create_dir_all(&dir.join("data")).unwrap();
    let effective = Effective::load(&Locations::under_home(dir), &SystemFiles, &[]);
    let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.join("home")));
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_clock(Arc::new(clock.clone()))
        .with_ids(Box::new(crate::SequentialIds::with_run(run)));
    match scan {
        Some(scan) => config.with_scratch_scan(scan),
        None => config,
    }
}

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

type Started = JoinHandle<Result<(HostHandle, HostJoin), clauseo_protocol::HostError>>;

/// Starts a host whose Scratch scan is held: `Host::start` runs on its own thread, as it returns
/// only once ready. Returns once the scan is entered and the ready wait armed.
fn start_held(dir: &Path, clock: &ManualClock) -> (Started, Held) {
    let (scan, held) = held_scan();
    let config = config(dir, clock, 0, Some(scan));
    let started = std::thread::spawn(move || Host::start(config));
    held.entered.recv_timeout(PATIENCE).unwrap();
    let armed = clock.clone();
    block_on(async move { while armed.next_armed().await != READY_WAIT {} });
    (started, held)
}

/// Passes the ready wait and takes the started host.
fn ready_past_the_wait(started: Started, clock: &ManualClock) -> (HostHandle, HostJoin) {
    assert!(!started.is_finished(), "ready went out with the scan held");
    clock.advance(READY_WAIT);
    started.join().unwrap().unwrap()
}

fn scratch_matters(host: &HostHandle) -> usize {
    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListMatters));
    let QueryResult::Matters(matters) = listed.unwrap() else {
        panic!("not matters");
    };
    matters
        .iter()
        .filter(|matter| matter.kind == MatterKind::Scratch)
        .count()
}

/// The Scratch rows in the next `MattersChanged` that has any, skipping other app events.
fn next_scratch_rows(app: &mut AppSubscription) -> usize {
    loop {
        let item = block_on(app.next()).expect("the app subscription ended");
        if let HostEvent::MattersChanged(changed) = item.decode().unwrap() {
            let rows = changed
                .matters
                .iter()
                .filter(|matter| matter.kind == MatterKind::Scratch)
                .count();
            if rows > 0 {
                return rows;
            }
        }
    }
}

fn scratch_folder(dir: &Path, name: &str) -> PathBuf {
    dir.join("Documents").join("Clauseo").join(name)
}

#[test]
fn ready_does_not_wait_for_a_held_scan_and_the_late_scratch_reaches_the_app() {
    let dir = TempDir::new("scratch-late");
    let clock = clock();
    let (started, held) = start_held(dir.path(), &clock);
    let (host, join) = ready_past_the_wait(started, &clock);
    assert_eq!(
        scratch_matters(&host),
        0,
        "no Scratch before its scan answers"
    );

    // Subscribed before the late Scratch: it hears of it as of any new matter.
    let mut app = block_on(host.subscribe_app(Principal::LocalDesktop)).unwrap();
    held.release.send(()).unwrap();
    let found = held.answered.recv_timeout(PATIENCE).unwrap();
    assert_eq!(found, Some(scratch_folder(dir.path(), "Scratch")));
    assert_eq!(next_scratch_rows(&mut app), 1);
    assert_eq!(scratch_matters(&host), 1);
    assert!(scratch_folder(dir.path(), "Scratch").is_dir());
    assert!(!scratch_folder(dir.path(), "Scratch 2").exists());
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();

    // A second start finds it: no second Scratch.
    let (host, join) = Host::start(config(dir.path(), &clock, 1, None)).unwrap();
    assert_eq!(scratch_matters(&host), 1);
    assert!(!scratch_folder(dir.path(), "Scratch 2").exists());
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn a_host_stopped_mid_scan_makes_no_scratch() {
    let dir = TempDir::new("scratch-stopped");
    let clock = clock();
    let (started, held) = start_held(dir.path(), &clock);
    let (host, join) = ready_past_the_wait(started, &clock);
    // Shutdown does not wait for the scan.
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();

    // The scan answers with the actor gone: nothing is made, and nothing panics.
    held.release.send(()).unwrap();
    assert!(held.answered.recv_timeout(PATIENCE).unwrap().is_some());
    assert!(!scratch_folder(dir.path(), "Scratch").exists());
    block_on(async {
        let store = Store::open(StoreOptions::new(dir.path().join("data")))
            .await
            .unwrap();
        assert!(
            store.matters().await.unwrap().is_empty(),
            "a matter was stored"
        );
        store.close().await.unwrap();
    });
}
