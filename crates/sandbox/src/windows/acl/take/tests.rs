//! `NtfsOwners` on a real NTFS volume, run as `--reclaim` runs, with its privileges: a junction
//! and a hard-linked file are skipped, never entered; a folder the owner owns but can't list
//! (its DACL shuts the owner out) is neither entered nor counted; and a
//! short (8.3) name or another case resolves, by handle, to the final path the bounds are
//! checked on.
#![expect(
    unsafe_code,
    reason = "FFI to GetShortPathNameW, for a test path's 8.3 name"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::GetShortPathNameW;
use windows::core::HSTRING;

use super::*;
use crate::test_support::TempDir;
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::reclaim::{Reclaimed, Scope, take_ownership};
use crate::windows::record::current_user_sid;
use crate::windows::setup::owner::Owner;
use crate::windows::test_dacl::{elevated, protect_with};
use crate::windows::test_junction::junction;
use crate::windows::test_world::OTHER_POOL_SIDS;

/// `path`'s short (8.3) form, when the volume made one.
fn short(path: &Path) -> Option<PathBuf> {
    let long = HSTRING::from(path.as_os_str());
    let mut buffer = vec![0_u16; 1024];
    // SAFETY: `long` is NUL-terminated and outlives the call; `buffer` is writable for its
    // length.
    let length = unsafe { GetShortPathNameW(&long, Some(&mut buffer)) } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    buffer.truncate(length);
    let short = PathBuf::from(OsString::from_wide(&buffer));
    (short != path).then_some(short)
}

/// Gives the folder back to its owner when dropped, so the temp folder can be removed even
/// when an assertion failed.
struct Reopen<'a>(&'a Path, &'a str);

impl Drop for Reopen<'_> {
    fn drop(&mut self) {
        drop(protect_with(
            self.0,
            &format!("D:P(A;OICI;FA;;;{})", self.1),
        ));
    }
}

/// Run as `--reclaim` runs, with its privileges (CI's elevated runner token holds them),
/// against the owner's own token taken before they are enabled, as the lawyer's has none.
#[test]
fn junctions_and_linked_files_are_skipped_and_a_shut_folder_isnt_entered() {
    if !elevated() {
        eprintln!("skipped: the token isn't elevated, so it holds no backup privilege");
        return;
    }
    let dir = TempDir::new("take-ntfs");
    let me = current_user_sid().unwrap();
    let matter = dunce::canonicalize(dir.dir("matter")).unwrap();
    let elsewhere = dunce::canonicalize(dir.dir("elsewhere")).unwrap();
    dir.file("elsewhere/beyond.txt", "x");
    junction(&matter.join("junction"), &elsewhere).unwrap();
    let original = dir.file("matter/a.txt", "x");
    dir.hard_link("matter/b.txt", &original);
    let shut = dir.dir("matter/shut");
    dir.file("matter/shut/inner.txt", "x");
    dir.file("matter/plain.txt", "x");
    // Shut: only SYSTEM is let in; the owner keeps no more than its implied rights.
    protect_with(&shut, "D:P(A;;FA;;;SY)").unwrap();
    let _reopen = Reopen(&shut, me.as_str());
    let lawyer = Owner::of_this_process().unwrap();
    let granted = |descriptor: &[u8]| {
        lawyer
            .granted(descriptor)
            .map_err(|e| std::io::Error::other(e.to_string()))
    };
    let pool = [Sid::parse(OTHER_POOL_SIDS[0]).unwrap()];
    let scope = Scope {
        pool: &pool,
        owner: &me,
        bounds: std::slice::from_ref(&matter),
        profiles: &[],
    };

    let seen = {
        let _privileges = Privileges::enable(&RECLAIM).unwrap();
        take_ownership(&mut NtfsOwners::new(&granted), &matter, true, &scope)
    };

    // Counted: the matter and plain.txt; skipped: the junction and both linked names; the shut
    // folder is passed over, uncounted, and nothing in it or beyond the junction is read.
    let want = Reclaimed {
        visited: 2,
        skipped: 3,
        ..Reclaimed::default()
    };
    assert_eq!(seen, want);
}

#[test]
fn a_short_name_or_another_case_resolves_to_the_final_path() {
    let dir = TempDir::new("take-ntfs-short");
    let long = dunce::canonicalize(dir.dir("a matter with a long name/sub")).unwrap();
    let lawyer = Owner::of_this_process().unwrap();
    let granted = |descriptor: &[u8]| {
        lawyer
            .granted(descriptor)
            .map_err(|e| std::io::Error::other(e.to_string()))
    };
    let mut tree = NtfsOwners::new(&granted);
    let resolved = tree.resolve(&long).unwrap();
    let upper = PathBuf::from(long.to_string_lossy().to_uppercase());
    assert_eq!(tree.resolve(&upper).unwrap(), resolved, "case");
    match short(&long) {
        Some(short) => assert_eq!(tree.resolve(&short).unwrap(), resolved, "{short:?}"),
        None => eprintln!("this volume makes no 8.3 names; the short-name case is skipped"),
    }
}
