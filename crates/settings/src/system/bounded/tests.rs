use super::{Unavailable, check, verified_dir_once};
use crate::locations::{KnownFolder, Locations, Os, PlatformDirs};
use crate::testing::FakeEnv;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_millis(300);

/// Blocks for `hang`, without sleeping: waits on a channel nobody sends on.
fn hang(hang: Duration) {
    let (_held, never) = mpsc::channel::<()>();
    assert!(never.recv_timeout(hang).is_err(), "nobody sends");
}

/// The folder the fake lookups name.
fn documents_path() -> PathBuf {
    PathBuf::from("/D/Documents")
}

/// `run`'s answer, and how long it took on the wall clock.
#[expect(
    clippy::disallowed_methods,
    reason = "these tests check a bound on wall-clock time"
)]
fn timed<T>(run: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let answer = run();
    (answer, started.elapsed())
}

#[test]
fn a_check_that_hangs_gives_no_folder_in_about_the_wait() {
    let (answer, took) = timed(|| {
        check(
            WAIT,
            || Some(documents_path()),
            |_: &Path| {
                hang(Duration::from_secs(30));
                true
            },
        )
    });
    assert_eq!(answer, Err(Unavailable::TimedOut));
    assert!(took >= WAIT, "{took:?}");
    assert!(took < WAIT * 2, "{took:?}");
}

#[test]
fn a_folder_that_is_there_is_given() {
    assert_eq!(
        check(WAIT, || Some(documents_path()), |_: &Path| true),
        Ok(documents_path())
    );
}

#[test]
fn a_missing_folder_or_lookup_gives_none() {
    assert_eq!(
        check(WAIT, || Some(documents_path()), |_: &Path| false),
        Err(Unavailable::Missing)
    );
    assert_eq!(
        check(WAIT, || None, |_: &Path| true),
        Err(Unavailable::Missing)
    );
    let panics = |_: &Path| -> bool { panic!("the check failed") };
    assert_eq!(
        check(WAIT, || Some(documents_path()), panics),
        Err(Unavailable::Missing)
    );
}

/// Callers during the wait join the first check, and later ones get its answer: one thread.
#[test]
fn the_check_runs_once_per_cell() {
    static CELL: OnceLock<Result<PathBuf, Unavailable>> = OnceLock::new();
    let runs = Arc::new(AtomicUsize::new(0));
    let callers: Vec<_> = (0..4)
        .map(|_| {
            let runs = Arc::clone(&runs);
            std::thread::spawn(move || {
                let lookup = move || {
                    runs.fetch_add(1, Ordering::SeqCst);
                    hang(Duration::from_millis(100));
                    Some(documents_path())
                };
                verified_dir_once(&CELL, "Documents", WAIT, lookup, |_: &Path| true)
            })
        })
        .collect();
    for caller in callers {
        assert_eq!(caller.join().unwrap(), Some(documents_path()));
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

/// Windows' Documents lookup as `SystemDirs` makes it, with a known-folder call that hangs
/// for 30 s: a share that is down.
struct HungShare;

impl PlatformDirs for HungShare {
    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/C/Users/ada"))
    }

    fn known_folder(&self, folder: KnownFolder) -> Option<PathBuf> {
        static DOCUMENTS: OnceLock<Result<PathBuf, Unavailable>> = OnceLock::new();
        match folder {
            KnownFolder::LocalAppData => Some(PathBuf::from("/C/Users/ada/AppData/Local")),
            KnownFolder::Documents => verified_dir_once(
                &DOCUMENTS,
                "Documents",
                super::DOCUMENTS_WAIT,
                || {
                    hang(Duration::from_secs(30));
                    Some(documents_path())
                },
                |_: &Path| true,
            ),
            _ => None,
        }
    }
}

/// The desktop's start: its main thread's path before the first window,
/// `Locations::resolve_without_documents`, answers at once while the lookup hangs; the
/// host-start thread's, `Locations::with_documents`, goes on without Documents after the 2 s
/// wait; and a later `Locations::resolve` does not wait again.
#[test]
fn a_hung_documents_lookup_holds_resolve_for_the_wait_only() {
    let env = FakeEnv::default();
    let (early, quick) =
        timed(|| Locations::without_documents_for(Os::Windows, &HungShare, &env).unwrap());
    assert_eq!(
        early.data_dir(),
        Path::new("/C/Users/ada/AppData/Local/ai.clauseo.desktop")
    );
    assert_eq!(early.default_matters_dir(), None);
    assert!(quick < Duration::from_millis(100), "{quick:?}");
    let (first, took) = timed(|| early.with_documents_for(Os::Windows, &HungShare, &env));
    assert_eq!(first.default_matters_dir(), None);
    assert_eq!(
        first.data_dir(),
        Path::new("/C/Users/ada/AppData/Local/ai.clauseo.desktop")
    );
    assert!(took >= super::DOCUMENTS_WAIT, "{took:?}");
    assert!(took < Duration::from_secs(3), "{took:?}");
    let (second, again) =
        timed(|| Locations::resolve_for(Os::Windows, &HungShare, &FakeEnv::default()).unwrap());
    assert_eq!(second, first);
    assert!(again < Duration::from_millis(100), "{again:?}");
}
