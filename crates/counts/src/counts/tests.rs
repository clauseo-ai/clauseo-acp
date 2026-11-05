//! The two mutexes (the module doc): a `bump` never waits on a file change under way, and an off
//! waits for one, then deletes the folder, so nothing written before it is left.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use super::Counts;
use crate::about::About;
use crate::schema::{
    AppOpened, Arch, Channel, Event, InstallType, Language, Launch, Platform, Time, Version,
};

const COLD: Event = Event::AppOpened(AppOpened {
    launch: Launch::Cold,
});

/// Long enough for a thread that is not blocked to finish on a loaded CI machine.
const PATIENCE: Duration = Duration::from_secs(20);

/// A data folder of this test's own under the temp folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("clauseo-counts-lock-{name}-{}", std::process::id()));
        drop(crate::write::remove_dir_all(&dir));
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(crate::write::remove_dir_all(&self.0));
    }
}

fn about() -> About {
    About {
        version: Version::new(3, 0, 4).unwrap(),
        channel: Channel::Stable,
        platform: Platform::macos(15).unwrap(),
        arch: Arch::Aarch64,
        language: Language::En,
        install_type: InstallType::PerUser,
        managed: false,
    }
}

/// 2026-11-04, `secs` after midnight UTC.
fn at(secs: u64) -> Time {
    Time::from_unix_secs(1_793_750_400 + secs)
}

/// Saving counts whose next file change stops until `release` gets a message, and says on
/// `entered` that it has stopped there. Changes after that one run straight through.
fn held(scratch: &Scratch) -> (Counts, Receiver<()>, Sender<()>) {
    let counts = Counts::open(&scratch.0, about(), true, true, 7);
    counts.tick(at(0)).unwrap();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let mut first = true;
    counts.disk().before_change = Some(Box::new(move || {
        if std::mem::take(&mut first) {
            let _sent = entered_tx.send(());
            let _released = release_rx.recv();
        }
    }));
    (counts, entered, release)
}

#[test]
fn bump_does_not_wait_for_a_tick_writing_files() {
    let scratch = Scratch::new("bump");
    let (counts, entered, release) = held(&scratch);
    counts.bump(COLD);
    let ticker = counts.clone();
    let tick = std::thread::spawn(move || ticker.tick(at(60)));
    entered
        .recv_timeout(PATIENCE)
        .expect("the tick reached its file write");

    // The tick is inside a file write now, and holds the disk mutex.
    let bumper = counts.clone();
    let (done_tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        bumper.bump(COLD);
        drop(done_tx.send(bumper.today()));
    });
    let today = done
        .recv_timeout(PATIENCE)
        .expect("bump and today returned while the tick was writing");
    assert_eq!(today.app_opened.len(), 1);

    release.send(()).unwrap();
    tick.join().unwrap().unwrap();
    // The second bump came after the tick took its snapshot: the next tick saves it.
    counts.tick(at(120)).unwrap();
}

#[test]
fn an_off_waits_for_a_write_under_way_then_deletes_it() {
    let scratch = Scratch::new("off");
    let (counts, entered, release) = held(&scratch);
    counts.bump(COLD);
    let ticker = counts.clone();
    let tick = std::thread::spawn(move || ticker.tick(at(60)));
    entered
        .recv_timeout(PATIENCE)
        .expect("the tick reached its file write");

    let switcher = counts.clone();
    let (off_tx, off) = mpsc::channel();
    std::thread::spawn(move || drop(off_tx.send(switcher.set_enabled(false))));
    assert_eq!(
        off.recv_timeout(Duration::from_millis(300)).err(),
        Some(RecvTimeoutError::Timeout),
        "the off waits for the write under way"
    );

    release.send(()).unwrap();
    tick.join().unwrap().unwrap();
    off.recv_timeout(PATIENCE).unwrap().unwrap();
    assert!(
        !scratch.0.join("counts").exists(),
        "the off deleted what the tick wrote"
    );
    // A tick after the off writes nothing.
    counts.bump(COLD);
    counts.tick(at(180)).unwrap();
    assert!(!scratch.0.join("counts").exists());
}
