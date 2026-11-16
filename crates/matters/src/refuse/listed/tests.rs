use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;

use super::super::check_folder_with;
use super::{Canonicalize, RESOLVE_WAIT, keep, resolved_entries};
use crate::test_dir::TestDir;
use crate::{Error, Refusal, write};

/// How long the hung canonicalise holds: far past the wait and the bound below.
const HANG: Duration = Duration::from_secs(30);

/// The bound each call must answer within: the wait and a second to spare.
const BOUND: Duration = Duration::from_secs(3);

/// A canonicalise that hangs for [`HANG`] on `hung`, held by a channel whose sender the test
/// keeps (nothing sleeps), and is `dunce::canonicalize` for every other path. Dropping the
/// sender lets the hung threads go.
fn hanging_on(hung: &Path) -> (Canonicalize, mpsc::Sender<()>) {
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let hung = hung.to_path_buf();
    let canonicalize: Canonicalize = Arc::new(move |path: &Path| {
        if path == hung {
            let held = held.lock().unwrap();
            assert!(held.recv_timeout(HANG).is_err(), "the test sends nothing");
            return Err(std::io::Error::other("the share did not answer"));
        }
        dunce::canonicalize(path)
    });
    (canonicalize, release)
}

/// A canonicalise that answers `real` for `hung` once the test sends on the returned channel (or
/// drops it), and is `dunce::canonicalize` for every other path.
fn answering_late_on(hung: &Path, real: PathBuf) -> (Canonicalize, mpsc::Sender<()>) {
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let hung = hung.to_path_buf();
    let canonicalize: Canonicalize = Arc::new(move |path: &Path| {
        if path == hung {
            let _released = held.lock().unwrap().recv();
            return Ok(real.clone());
        }
        dunce::canonicalize(path)
    });
    (canonicalize, release)
}

/// The path `entry` is compared by in the cache, which must hold it.
fn stored(entry: &PathBuf) -> PathBuf {
    let cache = super::RESOLVED.lock().unwrap();
    cache.as_ref().unwrap()[entry].path.clone()
}

/// `run`'s answer, or a panic when it takes longer than [`BOUND`]: it runs on a thread of its
/// own and the test waits on a channel, so no clock is read.
fn within_bound<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || drop(sender.send(run())));
    receiver
        .recv_timeout(BOUND)
        .expect("the call answers within 3 s")
}

/// Folders under `dir`: a refused folder whose canonicalise hangs (`hung`, inside `outer`), a
/// reachable refused folder, and a reachable folder that is none of them.
fn layout(dir: &TestDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let outer = dir.path().join("Roaming parent");
    let hung = outer.join("Roaming");
    let listed = dir.path().join("Listed");
    let fine = dir.path().join("Clients").join("Smith");
    for folder in [&hung, &listed, &fine] {
        write::create_dir_all(folder).unwrap();
    }
    (outer, hung, listed, fine)
}

#[test]
fn a_refused_folder_that_hangs_holds_the_resolve_to_the_wait() {
    let dir = TestDir::new("listed-hang-resolve");
    let (_, hung, listed, _) = layout(&dir);
    let (canonicalize, release) = hanging_on(&hung);
    let refused = vec![hung.clone(), listed.clone()];
    let resolved = within_bound(move || resolved_entries(&refused, &canonicalize, RESOLVE_WAIT));
    // The hung entry as written; the reachable one canonical (the test folder is already).
    assert_eq!(resolved, vec![hung, listed]);
    drop(release);
}

#[test]
fn a_place_check_answers_within_the_bound_while_a_refused_folder_hangs() {
    let dir = TestDir::new("listed-hang-check");
    let (outer, hung, listed, fine) = layout(&dir);
    let (canonicalize, release) = hanging_on(&hung);
    let refused = vec![hung.clone(), listed];
    let check = |folder: PathBuf| {
        let (refused, canonicalize) = (refused.clone(), Arc::clone(&canonicalize));
        within_bound(move || check_folder_with(&folder, &refused, &canonicalize))
    };
    // The first check pays the wait once, and a reachable folder is accepted.
    check(fine.clone()).unwrap();
    // The hung folder by its path as written, and a folder holding it, are still refused.
    for folder in [hung.clone(), outer] {
        match check(folder.clone()) {
            Err(Error::FolderRefused {
                reason: Refusal::Listed(entry),
                ..
            }) => assert_eq!(entry, hung, "{}", folder.display()),
            other => panic!("{} is refused, got {other:?}", folder.display()),
        }
    }
    // A folder inside a listed one can be a matter, as before.
    write::create_dir_all(&hung.join("Clients")).unwrap();
    check(hung.join("Clients")).unwrap();
    check(fine).unwrap();
    drop(release);
}

#[test]
fn a_refused_folder_is_canonicalised_once() {
    let dir = TestDir::new("listed-once");
    let (_, _, listed, _) = layout(&dir);
    let calls = Arc::new(Mutex::new(0_u32));
    let counted = Arc::clone(&calls);
    let canonicalize: Canonicalize = Arc::new(move |path: &Path| {
        *counted.lock().unwrap() += 1;
        dunce::canonicalize(path)
    });
    let refused = vec![listed.clone()];
    for _ in 0..3 {
        assert_eq!(
            resolved_entries(&refused, &canonicalize, RESOLVE_WAIT),
            vec![listed.clone()]
        );
    }
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn two_first_checks_at_once_canonicalise_a_hung_folder_once() {
    let dir = TestDir::new("listed-joiners");
    let (_, hung, listed, fine) = layout(&dir);
    let (hanging, release) = hanging_on(&hung);
    let calls = Arc::new(Mutex::new(0_u32));
    let (counted, hung_path) = (Arc::clone(&calls), hung.clone());
    let canonicalize: Canonicalize = Arc::new(move |path: &Path| {
        if path == hung_path {
            *counted.lock().unwrap() += 1;
        }
        hanging(path)
    });
    let refused = vec![hung, listed];
    let start = Arc::new(Barrier::new(2));
    let (sender, receiver) = mpsc::channel();
    for _ in 0..2 {
        let (refused, canonicalize, fine) =
            (refused.clone(), Arc::clone(&canonicalize), fine.clone());
        let (start, sender) = (Arc::clone(&start), sender.clone());
        std::thread::spawn(move || {
            start.wait();
            drop(sender.send(check_folder_with(&fine, &refused, &canonicalize)));
        });
    }
    for _ in 0..2 {
        let answer = receiver
            .recv_timeout(BOUND)
            .expect("each check answers within 3 s");
        answer.unwrap();
    }
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "the hung folder is canonicalised once"
    );
    drop(release);
}

#[test]
fn a_late_answer_replaces_the_as_written_fallback() {
    let dir = TestDir::new("listed-late");
    let (_, hung, listed, _) = layout(&dir);
    let real = dir.path().join("Roaming canonical");
    let (canonicalize, release) = answering_late_on(&hung, real.clone());
    let refused = vec![hung.clone(), listed.clone()];
    let (early, first) = (refused.clone(), Arc::clone(&canonicalize));
    let resolved = within_bound(move || resolved_entries(&early, &first, RESOLVE_WAIT));
    // The first check answers as written: the hung entry did not answer within the wait.
    assert_eq!(resolved, vec![hung.clone(), listed.clone()]);
    release.send(()).unwrap();
    // The late answer is stored on its own thread, so the test polls the cache, bounded by
    // `within_bound`; a known entry is never canonicalised again while it waits.
    let (later, expected) = (refused, real.clone());
    let answered = within_bound(move || {
        loop {
            let now = resolved_entries(&later, &canonicalize, RESOLVE_WAIT);
            if now[0] == expected {
                return now;
            }
            std::thread::yield_now();
        }
    });
    assert_eq!(answered, vec![real, listed]);
}

#[test]
fn a_late_answer_and_the_first_insert_end_canonical_in_either_order() {
    let real = PathBuf::from("/late-order/real");
    let as_written = PathBuf::from("/late-order/as-written");
    // The late answer lands before the first check's insert: the insert leaves it.
    let (before, after) = (
        PathBuf::from("/late-order/before"),
        PathBuf::from("/late-order/after"),
    );
    keep(&before, real.clone(), false);
    keep(&before, as_written.clone(), true);
    assert_eq!(stored(&before), real);
    // The late answer lands after the insert: it replaces the fallback.
    keep(&after, as_written, true);
    keep(&after, real.clone(), false);
    assert_eq!(stored(&after), real);
}

#[test]
fn a_canonical_answer_is_never_replaced() {
    let real = PathBuf::from("/late-keep/real");
    let entry = PathBuf::from("/late-keep/entry");
    keep(&entry, real.clone(), false);
    keep(&entry, PathBuf::from("/late-keep/as-written"), true);
    keep(&entry, PathBuf::from("/late-keep/other"), false);
    assert_eq!(stored(&entry), real);
}
