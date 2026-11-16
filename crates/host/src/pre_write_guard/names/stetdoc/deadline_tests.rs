//! The fence's own time limit, on a clock the test moves: a walk that never ends is refused at
//! the deadline; one that ends gives its own verdict.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use clauseo_protocol::Timestamp;

use super::super::reads::{Fenced, GuardFence, fenced_read};
use super::{WALK_DEADLINE, within};
use crate::agent::loads::GUARD_TIMEOUT_SECONDS;
use crate::clock::ManualClock;
use crate::network_paths::resolve::{Disk, Entry, Fs};
use crate::network_paths::tests::plain_net;
use crate::pre_write_guard::words::Escape;

/// A disk whose every look waits until the test drops the gate's sender: a mount that hangs.
struct Hung(Mutex<Receiver<()>>);

impl Hung {
    fn wait<T>(&self) -> io::Result<T> {
        let gate = self.0.lock().unwrap();
        assert!(
            gate.recv().is_err(),
            "nothing is sent; the sender is only dropped"
        );
        Err(io::ErrorKind::NotFound.into())
    }
}

impl Fs for Hung {
    fn entry(&self, _path: &Path) -> io::Result<Entry> {
        self.wait()
    }
    fn read_link(&self, _path: &Path) -> io::Result<PathBuf> {
        self.wait()
    }
    fn identity(&self, _path: &Path) -> io::Result<(u64, u128)> {
        self.wait()
    }
}

fn no_long_name(_: &Path) -> Option<PathBuf> {
    None
}

/// A walk on a disk that never answers is refused when the clock reaches the deadline, which is
/// [`WALK_DEADLINE`]; the guard doesn't wait for the walk.
#[test]
fn a_walk_that_never_ends_is_refused_at_the_deadline() {
    let (release, gate) = mpsc::channel::<()>();
    let root = std::env::temp_dir();
    let fence = GuardFence {
        data: vec![root.join("d")],
        ..GuardFence::default()
    };
    let matter = root.join("m");
    let read = move || {
        let hung = Hung(Mutex::new(gate));
        let net = plain_net(cfg!(windows));
        let disk = Disk {
            fs: &hung,
            real: &no_long_name,
            net: &net,
        };
        let command = "stetdoc view Contract.docx";
        fenced_read(
            command,
            Escape::Backslash,
            (&matter, &matter),
            &fence,
            &disk,
        )
    };
    let clock = ManualClock::new(Timestamp::ZERO);
    let mover = clock.clone();
    let deadline = std::thread::spawn(move || {
        futures::executor::block_on(async {
            let armed = mover.next_armed().await;
            mover.advance(armed);
            armed
        })
    });
    assert_eq!(within(&clock, read), Some(Fenced::Late));
    assert_eq!(deadline.join().unwrap(), WALK_DEADLINE);
    drop(release);
}

/// A walk that ends in time gives its own verdict, a refusal or none; one that fails is refused.
#[test]
fn a_walk_that_ends_in_time_gives_its_verdict() {
    let clock = ManualClock::new(Timestamp::ZERO);
    assert_eq!(within(&clock, || Some(Fenced::Shape)), Some(Fenced::Shape));
    assert_eq!(within(&clock, || None), None);
    let failed = || -> Option<Fenced> { panic!("the walk failed") };
    assert_eq!(within(&clock, failed), Some(Fenced::Late));
}

/// The deadline leaves time inside the limit Claude Code gives the hook, past which it would
/// run the command unchecked.
#[test]
fn the_deadline_is_well_inside_the_hooks_limit() {
    let spare = Duration::from_secs(GUARD_TIMEOUT_SECONDS).saturating_sub(WALK_DEADLINE);
    assert!(spare >= Duration::from_secs(2), "{spare:?}");
}
