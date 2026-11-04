//! The log file at its seams: rotation on real files, a full disk (`/dev/full`), a sink that
//! never returns, a rotation that fails, the filter, the panic hook and [`Cause`].

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use tracing::Dispatch;

use super::writer::BACKOFF;
use super::{Cause, FileSink, LogGuard, Sink, build, kept_target, panic};
use crate::clock::{Clock, SystemClock};
use crate::pipeline::tests::support::TempDir;

/// The events the full-disk and blocked-sink tests emit, and how long the caller may take.
const EVENTS: u64 = 10_000;
const CALLER_LIMIT: Duration = Duration::from_millis(200);

fn now() -> Duration {
    SystemClock.now().mono
}

fn real_time() -> Box<dyn Fn() -> Duration + Send> {
    Box::new(now)
}

/// Emits `count` warn events on this thread through `dispatch`; returns how long it took.
fn emit(dispatch: &Dispatch, count: u64) -> Duration {
    tracing::dispatcher::with_default(dispatch, || {
        let start = now();
        for n in 0..count {
            tracing::warn!(n, "a test line");
        }
        now().saturating_sub(start)
    })
}

/// Waits until the writer has taken every line callers put on the channel.
fn drained(guard: &LogGuard) {
    let shared = Arc::clone(guard.shared.as_ref().unwrap());
    let target = shared.sent.load(Ordering::SeqCst);
    let taken = shared.taken.lock().unwrap();
    drop(shared.progress.wait_while(taken, |taken| *taken < target));
}

/// Drops `guard`; returns how long the drop took.
fn drop_timed(guard: LogGuard) -> Duration {
    let start = now();
    drop(guard);
    now().saturating_sub(start)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn rotation_keeps_three_files_each_within_the_cap_and_drops_the_oldest() {
    let dir = TempDir::new("log-rotation");
    let cap = 2_000;
    let (dispatch, guard) = build(FileSink::new(dir.path().to_owned()), cap, real_time()).unwrap();
    // About 100 bytes a line: 400 lines are 20 caps' worth, so the first lines are long gone.
    emit(&dispatch, 400);
    assert_eq!(guard.dropped(), 0);
    drop(guard);
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["host.1.log", "host.2.log", "host.log"]);
    let mut all = String::new();
    for name in &names {
        let len = std::fs::metadata(dir.path().join(name)).unwrap().len();
        assert!(len <= cap, "{name} holds {len} bytes, over the cap {cap}");
        assert!(len > 0, "{name} is empty");
        all.push_str(&read(&dir.path().join(name)));
    }
    assert!(!all.contains(" n=0\n"), "the oldest line is still on disk");
    assert!(
        all.contains(" n=399\n"),
        "the newest line is missing:\n{all}"
    );
    let newest = read(&dir.path().join("host.log"));
    assert!(newest.contains("WARN clauseo_host::log_file::tests: a test line n=399"));
}

/// The sink of a file opened as it is, with no rotation: `/dev/full`.
#[cfg(target_os = "linux")]
struct RawFile(std::fs::File);

#[cfg(target_os = "linux")]
impl Sink for RawFile {
    fn len(&mut self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.0, bytes)
    }

    fn rotate(&mut self) -> io::Result<()> {
        Err(io::Error::other("no rotation"))
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_full_disk_never_stalls_the_caller_and_counts_its_drops() {
    let full = crate::write::open_append(Path::new("/dev/full")).unwrap();
    let (dispatch, guard) = build(RawFile(full), u64::MAX, real_time()).unwrap();
    let took = emit(&dispatch, EVENTS);
    assert!(took < CALLER_LIMIT, "{EVENTS} events took {took:?}");
    // Once the writer has taken every queued line, each event is counted: dropped by a caller
    // on a full channel, by the writer's ENOSPC, or within the backoff that follows it.
    drained(&guard);
    assert_eq!(guard.dropped(), EVENTS, "some lines were lost uncounted");
    assert!(drop_timed(guard) <= Duration::from_secs(1));
}

/// A sink whose first write never returns until the test lets it go.
struct Stuck(Mutex<mpsc::Receiver<()>>);

impl Sink for Stuck {
    fn len(&mut self) -> u64 {
        0
    }

    fn write(&mut self, _: &[u8]) -> io::Result<()> {
        if let Ok(release) = self.0.lock() {
            let _released = release.recv();
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_sink_that_never_returns_never_stalls_the_caller_or_the_exit() {
    let (release, released) = mpsc::channel();
    let (dispatch, guard) = build(Stuck(Mutex::new(released)), u64::MAX, real_time()).unwrap();
    let took = emit(&dispatch, EVENTS);
    assert!(took < CALLER_LIMIT, "{EVENTS} events took {took:?}");
    // The writer holds one batch in its stuck write; the channel holds 1024 more at most.
    assert!(
        guard.dropped() >= EVENTS - 1024 * 2,
        "{} dropped",
        guard.dropped()
    );
    let start = now();
    guard.flush();
    let took = now().saturating_sub(start);
    assert!(
        took <= Duration::from_secs(1),
        "the guard's flush took {took:?}"
    );
    let took = drop_timed(guard);
    assert!(
        took <= Duration::from_secs(1),
        "the guard's drop took {took:?}"
    );
    drop(release);
}

/// A file in memory whose rotation fails until `rotates` is set.
#[derive(Default)]
struct Held {
    current: Mutex<Vec<u8>>,
    rotated: Mutex<Vec<Vec<u8>>>,
    rotates: AtomicBool,
}

#[derive(Clone)]
struct HeldSink(Arc<Held>);

impl Sink for HeldSink {
    fn len(&mut self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.current.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if !self.0.rotates.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "held open"));
        }
        let old = std::mem::take(&mut *self.0.current.lock().unwrap());
        self.0.rotated.lock().unwrap().push(old);
        Ok(())
    }
}

#[test]
fn a_failed_rotation_stops_at_the_cap_counts_drops_and_retries_after_the_backoff() {
    let held = Arc::new(Held::default());
    let clock = Arc::new(AtomicU64::new(0));
    let time = {
        let clock = Arc::clone(&clock);
        Box::new(move || Duration::from_millis(clock.load(Ordering::SeqCst)))
    };
    let cap = 1_000;
    let (dispatch, guard) = build(HeldSink(Arc::clone(&held)), cap, time).unwrap();
    let flush = |dispatch: &Dispatch, count| {
        emit(dispatch, count);
        // Every line reaches the writer before the next step reads what it did.
        drained(&guard);
    };
    flush(&dispatch, 100);
    let written = held.current.lock().unwrap().len() as u64;
    assert!(written <= cap, "{written} bytes past the cap {cap}");
    assert!(written > cap / 2, "only {written} bytes before the cap");
    let dropped = guard.dropped();
    assert!(dropped > 0, "nothing counted");
    // Within the backoff nothing is tried, even once the file could rotate.
    held.rotates.store(true, Ordering::SeqCst);
    flush(&dispatch, 1);
    assert!(held.rotated.lock().unwrap().is_empty());
    assert_eq!(guard.dropped(), dropped + 1);
    // After it, the rotation is tried again; the new file starts with the drop count.
    clock.store(
        u64::try_from(BACKOFF.as_millis()).unwrap() + 1,
        Ordering::SeqCst,
    );
    flush(&dispatch, 1);
    assert_eq!(held.rotated.lock().unwrap().len(), 1);
    let current = String::from_utf8(held.current.lock().unwrap().clone()).unwrap();
    let mut lines = current.lines();
    let note = lines.next().unwrap();
    assert!(
        note.ends_with(&format!(
            "WARN clauseo_host::log_file: dropped {} lines",
            dropped + 1
        )),
        "{current}"
    );
    assert!(
        lines.next().unwrap().ends_with("a test line n=0"),
        "{current}"
    );
    assert_eq!(guard.dropped(), 0);
    drop(guard);
}

#[test]
fn the_file_keeps_warn_error_and_lifecycle_info_only() {
    use tracing::Level;
    for (level, target, kept) in [
        (Level::ERROR, "clauseo_matters::walk", true),
        (Level::WARN, "rustls", true),
        (Level::INFO, "clauseo_host::lifecycle", true),
        (Level::INFO, "clauseo_sandbox::lifecycle", true),
        (Level::INFO, "clauseo_host::actor::turn", false),
        (Level::INFO, "other::lifecycle", false),
        (Level::DEBUG, "clauseo_host::lifecycle", false),
        (Level::TRACE, "clauseo_host", false),
    ] {
        assert_eq!(kept_target(level, target), kept, "{level} {target}");
    }
}

#[test]
fn a_panic_writes_its_message_and_location() {
    let dir = TempDir::new("log-panic");
    panic::install(dir.path().to_owned());
    let line = line!() + 3;
    let panicked = std::thread::Builder::new()
        .name("log-panic-test".to_owned())
        .spawn(|| std::panic::panic_any("the test's own panic 4711"))
        .unwrap()
        .join();
    assert!(panicked.is_err());
    let log = read(&dir.path().join("host.log"));
    assert!(
        log.contains("ERROR clauseo_host::panic: a thread panicked"),
        "{log}"
    );
    assert!(
        log.contains("message=\"the test's own panic 4711\""),
        "{log}"
    );
    assert!(
        log.contains(&format!("location={}:{line}", file!())),
        "{log}"
    );
    assert!(log.contains("thread=\"log-panic-test\""), "{log}");
}

#[test]
fn cause_shows_codes_and_io_kinds_never_the_error_text() {
    let path = std::path::PathBuf::from("/Matters/Acme v Beta/Secret Memo.docx");
    let io = io::Error::from_raw_os_error(2);
    let matter = clauseo_matters::Error::Io {
        op: "read",
        path,
        source: io,
    };
    let error = crate::Error::Matters(matter);
    assert!(
        error.to_string().contains("Secret Memo"),
        "the fixture holds a path"
    );
    let shown = Cause(&error).to_string();
    assert_eq!(shown, "matter.checkpoint_failed io:NotFound/os:2");
    let plain = io::Error::other("Secret Memo.docx");
    assert_eq!(Cause(&plain).to_string(), "io:Other");
    let unknown = std::fmt::Error;
    assert_eq!(Cause(&unknown).to_string(), "unknown");
}
