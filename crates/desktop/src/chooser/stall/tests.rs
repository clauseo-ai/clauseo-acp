//! The pre-check with fake known folders and probes: local folders read no disk, a network
//! server that answers keeps the native dialog, one that hangs opens the chooser after the
//! shared 1.5 s, and a server still stuck gets no second probe.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::*;

/// Known folders at `paths`, Z a network drive; a probe of a path under `\\down` waits on
/// `gate` (up to 30 s), one under `\\broken` fails, and every other answers. Counts the probes.
struct Fake {
    paths: Vec<String>,
    probes: AtomicUsize,
    downs: AtomicUsize,
    drive_reads: AtomicUsize,
    gate: Mutex<mpsc::Receiver<()>>,
}

impl KnownFolders for Fake {
    fn paths(&self) -> Vec<String> {
        self.paths.clone()
    }
    fn remote_drive(&self, letter: char) -> bool {
        if letter == 'Y' {
            self.drive_reads.fetch_add(1, Ordering::SeqCst);
            let _released = self
                .gate
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30));
        }
        letter == 'Z'
    }
    fn answers(&self, path: &str) -> bool {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if path.starts_with(r"\\down") {
            self.downs.fetch_add(1, Ordering::SeqCst);
            let _released = self
                .gate
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30));
        }
        !path.starts_with(r"\\broken")
    }
}

fn known(paths: &[&str]) -> (Arc<Fake>, Arc<dyn KnownFolders>, mpsc::Sender<()>) {
    let (release, gate) = mpsc::channel();
    let fake = Arc::new(Fake {
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        probes: AtomicUsize::new(0),
        downs: AtomicUsize::new(0),
        drive_reads: AtomicUsize::new(0),
        gate: Mutex::new(gate),
    });
    let known: Arc<dyn KnownFolders> = Arc::clone(&fake) as Arc<dyn KnownFolders>;
    (fake, known, release)
}

/// Local known folders: native, and no probe at all.
#[test]
fn local_known_folders_probe_nothing() {
    let (fake, known, _release) = known(&[r"C:\Users\r\Documents", r"D:\Pictures"]);
    assert!(!Probes::default().would_stall(&known, PROBE_WAIT));
    assert_eq!(fake.probes.load(Ordering::SeqCst), 0);
}

/// Network folders that answer: native, one probe per server.
#[test]
fn network_folders_that_answer_keep_the_native_dialog() {
    let (fake, known, _release) = known(&[
        r"\\files\home\r\Documents",
        r"\\FILES\home\r\AppData\Roaming",
        r"Z:\Pictures",
        r"C:\Users\r\Desktop",
    ]);
    assert!(!Probes::default().would_stall(&known, PROBE_WAIT));
    assert_eq!(fake.probes.load(Ordering::SeqCst), 2);
}

/// A server that answers with an error opens the chooser.
#[test]
fn a_server_that_fails_opens_the_chooser() {
    let (_fake, known, _release) = known(&[r"\\broken\home\Documents"]);
    assert!(Probes::default().would_stall(&known, PROBE_WAIT));
}

/// A server that hangs opens the chooser after about 1.5 s, however many servers; a second
/// check while its probe is still stuck answers at once, with no second probe of it.
#[test]
fn a_hanging_server_opens_the_chooser_in_about_one_and_a_half_seconds() {
    let (fake, known, release) = known(&[r"\\down\home\Documents", r"\\files\home\Music"]);
    let probes = Probes::default();
    let started = crate::open::now();
    assert!(probes.would_stall(&known, PROBE_WAIT));
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(1400) && waited < Duration::from_secs(4),
        "{waited:?}"
    );
    let started = crate::open::now();
    assert!(probes.would_stall(&known, PROBE_WAIT));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(fake.downs.load(Ordering::SeqCst), 1);
    drop(release);
}

/// A drive whose type does not answer (a disconnected mapped drive `Y:`) opens the chooser after
/// about 1.5 s; a second check while that read is stuck answers at once, with no second read.
#[test]
fn a_hanging_drive_type_opens_the_chooser_in_about_one_and_a_half_seconds() {
    let (fake, known, release) = known(&[r"Y:\Documents"]);
    let probes = Probes::default();
    let started = crate::open::now();
    assert!(probes.would_stall(&known, PROBE_WAIT));
    let waited = crate::open::now().saturating_duration_since(started);
    assert!(
        waited >= Duration::from_millis(1400) && waited < Duration::from_secs(4),
        "{waited:?}"
    );
    let started = crate::open::now();
    assert!(probes.would_stall(&known, PROBE_WAIT));
    assert!(crate::open::now().saturating_duration_since(started) < Duration::from_millis(500));
    assert_eq!(fake.drive_reads.load(Ordering::SeqCst), 1);
    drop(release);
}

/// Off Windows the native dialog always; on Windows the variable decides first, then the check.
#[test]
fn the_route_follows_the_os_the_variable_then_the_check() {
    assert_eq!(route(false, false, Some("app"), || true), Route::Native);
    assert_eq!(route(false, true, Some("app"), || false), Route::Chooser);
    assert_eq!(route(false, true, Some("native"), || true), Route::Native);
    assert_eq!(route(false, true, None, || true), Route::Chooser);
    assert_eq!(route(false, true, Some("other"), || false), Route::Native);
}
