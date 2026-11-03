use std::os::unix::fs::{MetadataExt, PermissionsExt};

use super::*;
use crate::test_support::{TempDir, set_mode};

const A: &str = "aaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbb";

/// Yields `ids` in order, then fails.
fn ids(ids: &[&str]) -> impl FnMut() -> io::Result<String> {
    let mut left: Vec<String> = ids.iter().rev().map(|&id| id.to_owned()).collect();
    move || left.pop().ok_or_else(|| io::Error::other("out of ids"))
}

fn name(id: &str) -> String {
    format!("{PREFIX}{id}")
}

fn lock(id: &str) -> String {
    format!("{PREFIX}{id}{LOCK_SUFFIX}")
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[test]
fn a_made_folder_is_0700_ours_and_not_a_link() {
    let tmp = TempDir::new("browser-dir-made");
    let dir = BrowserSocketDir::create_in(tmp.path()).unwrap();
    let meta = std::fs::symlink_metadata(dir.path()).unwrap();
    assert!(meta.file_type().is_dir() && !meta.file_type().is_symlink());
    assert_eq!(meta.permissions().mode() & 0o7777, 0o700);
    assert_eq!(meta.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(dir.path(), tmp.path().join(name(dir.id())));
    assert_eq!(dir.canonical(), dunce::canonicalize(dir.path()).unwrap());
    assert!(id_of(&name(dir.id())).is_some(), "{}", dir.id());
    let lock_meta = std::fs::symlink_metadata(tmp.path().join(lock(dir.id()))).unwrap();
    assert!(lock_meta.file_type().is_file());
    assert_eq!(lock_meta.permissions().mode() & 0o7777, 0o600);
    assert_eq!(lock_meta.uid(), rustix::process::geteuid().as_raw());
}

#[test]
fn an_existing_lock_file_of_the_same_name_is_refused_and_left() {
    let tmp = TempDir::new("browser-dir-lock-taken");
    let taken = tmp.file(&lock(A), "someone's\n");
    let error = BrowserSocketDir::create_named_in(tmp.path(), A).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(!exists(&tmp.path().join(name(A))));
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "someone's\n");
    let dir = create_with(tmp.path(), ids(&[A, B])).unwrap();
    assert_eq!(dir.id(), B);
}

#[test]
fn ids_are_sixteen_letters_and_differ() {
    let (a, b) = (random_id().unwrap(), random_id().unwrap());
    assert_ne!(a, b);
    for id in [a, b] {
        assert!(id_of(&name(&id)).is_some(), "{id}");
    }
}

#[test]
fn drop_removes_the_folder_and_what_is_in_it() {
    let tmp = TempDir::new("browser-dir-drop");
    let dir = BrowserSocketDir::create_in(tmp.path()).unwrap();
    let path = dir.path().to_owned();
    tmp.file(&format!("{}/d.pid", name(dir.id())), "1\n");
    let lock_path = tmp
        .path()
        .join(format!("{PREFIX}{}{LOCK_SUFFIX}", dir.id()));
    drop(dir);
    assert!(
        std::fs::symlink_metadata(&lock_path).is_err(),
        "{}",
        lock_path.display()
    );
    assert!(
        std::fs::symlink_metadata(&path).is_err(),
        "{}",
        path.display()
    );
}

#[test]
fn an_existing_folder_of_the_same_name_is_refused_and_a_new_id_drawn() {
    let tmp = TempDir::new("browser-dir-taken");
    let taken = tmp.file(&format!("{}/mine.txt", name(A)), "someone's\n");
    let error = BrowserSocketDir::create_named_in(tmp.path(), A).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        !exists(&tmp.path().join(lock(A))),
        "our lock file was removed"
    );
    let dir = create_with(tmp.path(), ids(&[A, B])).unwrap();
    assert_eq!(dir.id(), B);
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "someone's\n");
}

#[test]
fn every_name_taken_is_an_error() {
    let tmp = TempDir::new("browser-dir-all-taken");
    tmp.dir(&name(A));
    let error = create_with(tmp.path(), ids(&[A; TRIES])).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_link_in_its_place_is_refused_and_left_with_its_target() {
    let tmp = TempDir::new("browser-dir-link");
    let target = tmp.dir("target");
    let inside = tmp.file("target/kept.txt", "kept\n");
    let link = tmp.symlink(&name(A), &target);
    let error = BrowserSocketDir::create_named_in(tmp.path(), A).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(verify(&link).is_err());
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink());
    assert_eq!(std::fs::read_link(&link).unwrap(), target);
    assert_eq!(std::fs::read_to_string(&inside).unwrap(), "kept\n");
}

#[test]
fn wrong_permissions_are_refused() {
    let tmp = TempDir::new("browser-dir-mode");
    let path = tmp.dir(&name(A));
    set_mode(&path, 0o755);
    let error = verify(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("mode 755"), "{error}");
    set_mode(&path, 0o700);
    verify(&path).unwrap();
}

#[test]
fn a_file_is_refused() {
    let tmp = TempDir::new("browser-dir-file");
    let path = tmp.file(&name(A), "");
    assert!(verify(&path).is_err());
}

#[test]
fn sweep_removes_only_our_stale_folders() {
    let tmp = TempDir::new("browser-dir-sweep");
    let stale = tmp.dir(&name(A));
    tmp.file(&format!("{}/d.sock", name(A)), "");
    tmp.file(&lock(A), "");
    let kept = tmp.dir(&name(B));
    let target = tmp.dir("target");
    let link = tmp.symlink(&name("cccccccccccccccc"), &target);
    let short = tmp.dir(&name("short"));
    let upper = tmp.dir(&name("EEEEEEEEEEEEEEEE"));
    let other = tmp.dir("clauseo-other-aaaaaaaaaaaaaaaa");
    let file = tmp.file(&name("dddddddddddddddd"), "a file\n");
    let removed = BrowserSocketDir::sweep_stale_in(tmp.path(), |id| id == B).unwrap();
    assert_eq!(removed, 1);
    assert!(std::fs::symlink_metadata(&stale).is_err());
    for left in [&kept, &link, &short, &upper, &other, &file, &target] {
        assert!(
            std::fs::symlink_metadata(left).is_ok(),
            "{}",
            left.display()
        );
    }
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn sweep_skips_a_folder_whose_lock_a_live_process_holds() {
    let tmp = TempDir::new("browser-dir-sweep-live");
    let live = BrowserSocketDir::create_in(tmp.path()).unwrap();
    assert_eq!(
        BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false).unwrap(),
        0
    );
    assert!(exists(live.path()));
    assert!(exists(&tmp.path().join(lock(live.id()))));
}

#[test]
fn sweep_removes_an_unlocked_stale_pair_and_a_lone_lock() {
    let tmp = TempDir::new("browser-dir-sweep-pair");
    let folder = tmp.dir(&name(A));
    let pair_lock = tmp.file(&lock(A), "");
    let lone = tmp.file(&lock(B), "");
    assert_eq!(
        BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false).unwrap(),
        1
    );
    for gone in [&folder, &pair_lock, &lone] {
        assert!(!exists(gone), "{}", gone.display());
    }
}

#[test]
fn a_lock_that_is_a_link_or_a_folder_keeps_its_folder_and_is_not_followed() {
    let tmp = TempDir::new("browser-dir-sweep-link");
    let target = tmp.file("target.lock", "target\n");
    let linked = tmp.dir(&name(A));
    let link = tmp.symlink(&lock(A), &target);
    let odd = tmp.dir(&name(B));
    let lock_folder = tmp.dir(&lock(B));
    assert_eq!(
        BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false).unwrap(),
        0
    );
    for left in [&linked, &link, &odd, &lock_folder, &target] {
        assert!(exists(left), "{}", left.display());
    }
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "target\n");
}

/// A hard link to a root-owned file is a lock file that is not ours. macOS only: Linux may
/// refuse the link (`protected_hardlinks`) or keep `/etc` on another file system.
#[cfg(target_os = "macos")]
#[test]
fn a_lock_that_is_not_ours_keeps_its_folder() {
    let tmp = TempDir::new("browser-dir-sweep-owner");
    let folder = tmp.dir(&name(A));
    let foreign = tmp.hard_link(&lock(A), Path::new("/private/etc/hosts"));
    assert_ne!(
        std::fs::symlink_metadata(&foreign).unwrap().uid(),
        rustix::process::geteuid().as_raw()
    );
    assert_eq!(
        BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false).unwrap(),
        0
    );
    assert!(exists(&folder));
    assert!(exists(&foreign));
}

#[test]
fn a_failure_after_mkdir_removes_the_folder_and_the_lock_file() {
    let tmp = TempDir::new("browser-dir-late-failure");
    let error = BrowserSocketDir::create_checked(tmp.path(), A, |path| {
        assert!(exists(path), "the folder was made before the check");
        Err(io::Error::other("injected"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "injected");
    assert!(!exists(&tmp.path().join(name(A))));
    assert!(!exists(&tmp.path().join(lock(A))));
}

#[test]
fn an_entry_that_cannot_be_removed_does_not_stop_the_sweep() {
    let tmp = TempDir::new("browser-dir-sweep-stuck");
    let stuck = tmp.dir(&name(A));
    tmp.file(&lock(A), "");
    tmp.file(&format!("{}/sub/held.txt", name(A)), "held\n");
    let sub = stuck.join("sub");
    set_mode(&sub, 0o500);
    let stale = tmp.dir(&name(B));
    tmp.file(&lock(B), "");
    let lone = tmp.file(&lock("cccccccccccccccc"), "");
    let removed = BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false);
    set_mode(&sub, 0o700);
    assert_eq!(removed.unwrap(), 1);
    assert!(exists(&stuck));
    assert!(!exists(&stale));
    assert!(!exists(&lone));
}

/// Creators and sweepers at once in one root: a sweep never removes a folder a creator holds,
/// and no create fails (a sweeper holding a lock for an instant only costs a new id).
#[test]
fn sweeps_never_remove_a_live_folder_and_creates_never_fail_under_contention() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    const CREATES: usize = 300;
    let tmp = TempDir::new("browser-dir-stress");
    let root = tmp.path();
    let done = AtomicBool::new(false);
    let finished = AtomicUsize::new(0);
    let failures: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..CREATES {
                    match BrowserSocketDir::create_in(root) {
                        Ok(dir) => {
                            let lock_path = root.join(lock(dir.id()));
                            if !exists(dir.path()) || !exists(&lock_path) {
                                failures
                                    .lock()
                                    .unwrap()
                                    .push(format!("swept: {}", dir.id()));
                            }
                        }
                        Err(error) => failures.lock().unwrap().push(format!("create: {error}")),
                    }
                }
                if finished.fetch_add(1, Ordering::SeqCst) == 3 {
                    done.store(true, Ordering::SeqCst);
                }
            });
        }
        for _ in 0..4 {
            scope.spawn(|| {
                while !done.load(Ordering::SeqCst) {
                    BrowserSocketDir::sweep_stale_in(root, |_| false).unwrap();
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} failures: {failures:?}",
        failures.len()
    );
}

#[test]
fn a_lock_file_replaced_before_its_identity_check_costs_a_new_id() {
    let tmp = TempDir::new("browser-dir-identity");
    let made = create_each(ids(&[A, B]), |id| {
        if id == A {
            BrowserSocketDir::create_hooked(
                tmp.path(),
                id,
                |lock_path| {
                    // A sweep unlinked it and another file took the name.
                    crate::test_support::remove(lock_path);
                    tmp.file(&lock(A), "another\n");
                },
                checked,
            )
        } else {
            BrowserSocketDir::create_named_in(tmp.path(), id)
        }
    })
    .unwrap();
    assert_eq!(made.id(), B);
    assert!(
        !exists(&tmp.path().join(name(A))),
        "no folder for the lost name"
    );
    let other = tmp.path().join(lock(A));
    assert_eq!(std::fs::read_to_string(other).unwrap(), "another\n");
}

#[test]
fn a_folder_whose_lock_file_is_missing_is_kept() {
    let tmp = TempDir::new("browser-dir-sweep-no-lock");
    let folder = tmp.dir(&name(A));
    assert_eq!(
        BrowserSocketDir::sweep_stale_in(tmp.path(), |_| false).unwrap(),
        0
    );
    assert!(exists(&folder));
}
