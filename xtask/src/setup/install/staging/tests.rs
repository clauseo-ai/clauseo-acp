//! Tests for `staging.rs`: the sweep removes a staging dir whose lock is free and leaves one whose
//! lock another run holds.

use super::*;
use crate::scratch::Scratch;

/// The names in `dir`, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_sweep_leaves_a_staging_dir_whose_lock_is_held() {
    let dir = Scratch::new("setup-staging-held");
    let bin = dir.join("bin");
    let (held, lock) = fresh_dir(&bin, "gitleaks-1").unwrap();
    fs::write(held.join("gitleaks.tar.gz"), b"half a download").unwrap();
    remove_staging_leftovers(&bin).unwrap();
    assert_eq!(
        fs::read(held.join("gitleaks.tar.gz")).unwrap(),
        b"half a download"
    );
    drop(lock);
}

#[test]
fn the_sweep_removes_a_staging_dir_whose_lock_is_free() {
    let dir = Scratch::new("setup-staging-free");
    let bin = dir.join("bin");
    let (_, lock) = fresh_dir(&bin, "gitleaks-1").unwrap();
    assert_eq!(names_in(&bin), [".xtask-setup-gitleaks-1"]);
    // The OS drops the lock when the run that holds it ends; dropping the file does the same.
    drop(lock);
    remove_staging_leftovers(&bin).unwrap();
    assert_eq!(names_in(&bin), Vec::<String>::new());
}
