//! A rewrite that fails leaves no temp file behind, whatever step fails and whatever the file's
//! or the folder's ACL denies: the temp file never carries the target's ACL (macOS copies it
//! after the rename), and one its own ACL keeps from being removed is given one that allows it
//! ([`super::remove_temp`]). A crash's leftovers go before the next write in the folder
//! ([`super::sweep_temps`]).

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::test_dir::TestDir;

/// `name` in the test folder, holding "old".
fn old_file(dir: &TestDir, name: &str) -> PathBuf {
    let target = dir.path().join(name);
    super::create_new(&target)
        .unwrap()
        .write_all(b"old")
        .unwrap();
    target
}

/// Rewrites `target` with "new" as a write-back does.
fn rewrite(target: &Path) -> Result<(), crate::Error> {
    let staged = super::stage(target, &mut |file: &mut File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })?;
    staged.rename().map(drop).map_err(|failed| failed.error)
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// `target` holds "new" after a rewrite that went, and one whole content after one that failed;
/// nothing else is in its folder.
fn only_the_target(target: &Path, result: &Result<(), crate::Error>) {
    let held = std::fs::read(target).unwrap();
    if result.is_ok() {
        assert_eq!(held, b"new");
    } else {
        assert!(held == b"old" || held == b"new", "{held:?}");
    }
    let name = target.file_name().unwrap().to_str().unwrap().to_owned();
    assert_eq!(names(target.parent().unwrap()), [name]);
}

#[test]
fn a_failure_at_any_step_leaves_no_temp_file() {
    let dir = TestDir::new("leftover-steps");
    let target = old_file(&dir, "Lease.docx");
    let failed = super::stage(&target, &mut |_: &mut File| {
        Err(crate::Error::io(
            "write",
            "x",
            std::io::ErrorKind::Other.into(),
        ))
    });
    assert!(failed.is_err());
    only_the_target(&target, &Err(failed.err().unwrap()));
    for step in ["keep", "sync", "rename"] {
        super::fail_at(Some(step));
        let result = rewrite(&target);
        super::fail_at(None);
        assert!(result.is_err(), "{step}");
        only_the_target(&target, &result);
    }
}

#[test]
fn temp_files_a_crash_left_go_before_the_next_write_there() {
    let dir = TestDir::new("leftover-crash");
    let target = old_file(&dir, "Lease.docx");
    for n in 0..3 {
        old_file(&dir, &format!(".clauseo-{n}.tmp"));
    }
    super::sweep_temps(dir.path());
    only_the_target(
        &target,
        &Err(crate::Error::io("x", "x", std::io::ErrorKind::Other.into())),
    );
}

/// The sweep takes every `.clauseo-<digits>.tmp` by its exact name, past gaps too: never a
/// Windows backup (`<name>.clauseo-<n>.orig.tmp`) or a lawyer's file with a like name.
#[test]
fn the_sweep_takes_only_its_own_temp_files() {
    let dir = TestDir::new("leftover-sweep-names");
    let kept = [
        "Lease.docx.clauseo-0.orig.tmp",
        "Lease.clauseo-1.tmp",
        ".clauseo-0.tmp.docx",
        ".clauseo-x.tmp",
        "clauseo-1.tmp",
        ".clauseo-.tmp",
    ];
    for name in kept.iter().chain(&[".clauseo-3.tmp", ".clauseo-12.tmp"]) {
        old_file(&dir, name);
    }
    super::sweep_temps(dir.path());
    let mut want: Vec<String> = kept.iter().map(|name| (*name).to_owned()).collect();
    want.sort();
    assert_eq!(names(dir.path()), want);
}

/// A read-only target: Unix rewrites it (the mode is the folder's business); Windows refuses
/// it, as a rename does. No temp file either way.
#[test]
fn a_read_only_target_leaves_no_temp_file() {
    let dir = TestDir::new("leftover-read-only");
    let target = old_file(&dir, "Lease.docx");
    let mut permissions = std::fs::metadata(&target).unwrap().permissions();
    let writable = permissions.clone();
    permissions.set_readonly(true);
    super::set_permissions(&target, permissions).unwrap();
    let result = rewrite(&target);
    assert_eq!(result.is_ok(), cfg!(unix), "{result:?}");
    only_the_target(&target, &result);
    super::set_permissions(&target, writable).unwrap();
}

/// macOS: a target whose ACL denies deleting it: the rename is refused, as main's plain rename
/// is (the temp file has no ACL then, so it is the target's deny that refuses), and the temp
/// file goes.
#[cfg(target_os = "macos")]
#[test]
fn a_deny_delete_target_is_refused_and_leaves_no_temp_file_on_macos() {
    use super::macos;
    let dir = TestDir::new("leftover-deny-file");
    let target = old_file(&dir, "Lease.docx");
    macos::deny(&target, macos::ACL_DELETE).unwrap();
    let result = rewrite(&target);
    let Err(crate::Error::Io { source, .. }) = &result else {
        panic!("not refused: {result:?}");
    };
    assert_eq!(
        source.kind(),
        std::io::ErrorKind::PermissionDenied,
        "{source}"
    );
    only_the_target(&target, &result);
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    macos::clear_acl(&target).unwrap();
}

/// macOS: a folder whose ACL denies deleting its files: the rename is refused, and the temp
/// file is given an ACL that lets it go.
#[cfg(target_os = "macos")]
#[test]
fn a_deny_delete_folder_leaves_no_temp_file_on_macos() {
    use super::macos;
    let dir = TestDir::new("leftover-deny-folder");
    let target = old_file(&dir, "Lease.docx");
    macos::deny(dir.path(), macos::ACL_DELETE_CHILD).unwrap();
    for step in [None, Some("rename")] {
        super::fail_at(step);
        let result = rewrite(&target);
        super::fail_at(None);
        only_the_target(&target, &result);
    }
    macos::clear_acl(dir.path()).unwrap();
    macos::clear_acl(&target).unwrap();
}

/// macOS: the original's ACL, the current user allowed `delete` and `everyone` denied it, is
/// exactly the rewritten file's (copied after the rename); read back as text, which also proves
/// the tag and permission values.
#[cfg(target_os = "macos")]
#[test]
fn a_rewrite_keeps_the_exact_acl_on_macos() {
    use super::macos;
    let dir = TestDir::new("leftover-acl-kept");
    let target = old_file(&dir, "Lease.docx");
    macos::allow_me_deny_everyone(&target).unwrap();
    let before = macos::acl_text(&File::open(&target).unwrap());
    let lines: Vec<&str> = before.lines().collect();
    assert_eq!(lines.len(), 3, "{before}");
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&target).unwrap());
    assert!(lines[1].starts_with("user:"), "{before}");
    assert!(
        lines[1].ends_with(&format!(":{uid}:allow:delete")),
        "{before}"
    );
    assert!(lines[2].ends_with(":everyone:12:deny:delete"), "{before}");
    rewrite(&target).unwrap();
    assert_eq!(macos::acl_text(&File::open(&target).unwrap()), before);
    only_the_target(&target, &Ok(()));
    macos::clear_acl(&target).unwrap();
}

/// macOS: the strip step's ACL is one entry, the current user allowed `delete`, nothing broader.
#[cfg(target_os = "macos")]
#[test]
fn the_strip_step_allows_only_the_user_to_delete_on_macos() {
    use super::macos;
    let dir = TestDir::new("leftover-strip-acl");
    let target = old_file(&dir, "Lease.docx");
    macos::allow_delete(&target).unwrap();
    let text = macos::acl_text(&File::open(&target).unwrap());
    let lines: Vec<&str> = text.lines().collect();
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&target).unwrap());
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0], "!#acl 1", "{text}");
    assert!(lines[1].starts_with("user:"), "{text}");
    assert!(
        lines[1].ends_with(&format!(":{uid}:allow:delete")),
        "{text}"
    );
    macos::clear_acl(&target).unwrap();
}

/// Windows: a target whose ACL denies deleting it, and a folder that denies deleting its files
/// and hands that deny to new files: whatever `ReplaceFileW` and the rename make of them, and
/// with the rename failing too, no temp file is left.
#[cfg(windows)]
#[test]
fn a_deny_delete_file_or_folder_leaves_no_temp_file_on_windows() {
    use super::windows::set_dacl;
    let dir = TestDir::new("leftover-deny-windows");
    let target = old_file(&dir, "Lease.docx");
    set_dacl(&target, "D:P(D;;SD;;;WD)(A;;FA;;;WD)").unwrap();
    for step in [None, Some("rename")] {
        super::fail_at(step);
        let result = rewrite(&target);
        super::fail_at(None);
        only_the_target(&target, &result);
    }
    set_dacl(&target, "D:P(A;;FA;;;WD)").unwrap();
    set_dacl(
        dir.path(),
        "D:P(D;OICI;SD;;;WD)(D;;DC;;;WD)(A;OICI;FA;;;WD)",
    )
    .unwrap();
    for step in [None, Some("rename")] {
        super::fail_at(step);
        let result = rewrite(&target);
        super::fail_at(None);
        only_the_target(&target, &result);
    }
    set_dacl(dir.path(), "D:P(A;OICI;FA;;;WD)").unwrap();
    set_dacl(&target, "D:P(A;;FA;;;WD)").unwrap();
}
