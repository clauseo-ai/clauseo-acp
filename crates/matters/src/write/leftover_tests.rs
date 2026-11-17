//! A rewrite that fails leaves no temp file behind, whatever step fails and whatever the file's
//! or the folder's ACL denies: the temp file never carries the target's ACL (macOS sets it after
//! the rename), and one its own ACL keeps from being removed is given one that allows it, through
//! its own handle ([`super::temps`]). A crash's leftovers go before the next write in the folder
//! ([`super::sweep_temps`]), and nothing at a temp name is ever unlocked but the write's own file:
//! never a lawyer's file linked there, never a junction's target.

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

/// A failure after the rename (before the ACL and the mode, on macOS) fails nothing: the new
/// content is in place, no temp file is left, and the rename reports the file as its owner's
/// alone, with one thing lost. macOS: the mode is the owner-only one.
#[test]
fn a_failure_after_the_rename_leaves_no_temp_file_and_names_the_file() {
    let dir = TestDir::new("leftover-after-rename");
    let target = old_file(&dir, "Lease.docx");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        super::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let staged = super::stage(&target, &mut |file: &mut File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    super::fail_at(Some("after rename"));
    let renamed = staged.rename();
    super::fail_at(None);
    let renamed = renamed.unwrap();
    assert!(renamed.owner_only);
    assert_eq!(renamed.lost.count, 1);
    only_the_target(&target, &Ok(()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        // Linux sets the mode before the rename; macOS only after it, with the ACL.
        let want = if cfg!(target_os = "macos") {
            0o600
        } else {
            0o640
        };
        assert_eq!(mode, want, "{mode:o}");
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
    macos::allow_delete(&File::open(&target).unwrap()).unwrap();
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

/// What a lawyer's file has that an unlock would change: macOS its ACL as text and its mode;
/// Windows its DACL as SDDL and its attributes; Linux its mode and a user attribute (none on a
/// file system without them). Its content too.
fn fingerprint(path: &Path) -> String {
    let content = String::from_utf8(std::fs::read(path).unwrap()).unwrap();
    #[cfg(target_os = "macos")]
    let rest = {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        format!(
            "{mode:o} {}",
            super::macos::acl_text(&File::open(path).unwrap())
        )
    };
    #[cfg(target_os = "linux")]
    let rest = {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        let mut value = [0_u8; 64];
        let attr = rustix::fs::getxattr(path, "user.clauseo.test", &mut value[..])
            .map(|len| value.get(..len).unwrap_or_default().to_vec());
        format!("{mode:o} {attr:?}")
    };
    #[cfg(windows)]
    let rest = {
        use std::os::windows::fs::MetadataExt as _;
        /// `FILE_ATTRIBUTE_ARCHIVE`: NTFS sets it itself when the file's record changes, as when
        /// the test adds a hard link to it; no call of Clauseo's sets it. The unlock would set
        /// `FILE_ATTRIBUTE_NORMAL`, which clears the hidden bit kept here.
        const ARCHIVE: u32 = 0x20;
        let attributes = std::fs::symlink_metadata(path).unwrap().file_attributes() & !ARCHIVE;
        format!("{attributes:x} {}", super::windows::dacl_text(path))
    };
    format!("{content} {rest}")
}

/// Gives the lawyer's file `path` what an agent would want stripped: macOS `everyone` denied
/// delete and a group denied read; Windows the same in its DACL, and the hidden attribute;
/// Linux mode 0640 and a user attribute.
fn protect(path: &Path) {
    #[cfg(target_os = "macos")]
    super::macos::deny_delete_and_read(path).unwrap();
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt as _;
        super::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
        // A file system without user attributes leaves the mode alone to check.
        if let Err(err) = rustix::fs::setxattr(
            path,
            "user.clauseo.test",
            b"kept",
            rustix::fs::XattrFlags::empty(),
        ) {
            eprintln!("no user attribute ({err}): the mode alone is checked");
        }
    }
    #[cfg(windows)]
    {
        super::windows::set_dacl(path, "D:P(D;;SD;;;WD)(D;;FR;;;BG)(A;;FA;;;WD)").unwrap();
        super::windows::set_attributes(path, 0x2).unwrap();
    }
}

/// Undoes [`protect`], so the test folder can go.
fn unprotect(path: &Path) {
    #[cfg(target_os = "macos")]
    super::macos::clear_acl(path).unwrap();
    #[cfg(windows)]
    {
        super::windows::set_dacl(path, "D:P(A;;FA;;;WD)").unwrap();
        super::windows::set_attributes(path, 0x80).unwrap();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = path;
}

/// An agent's hard link to a lawyer's file, at a temp name: a crash's leftover name the sweep
/// meets before a write, and the write's own temp file swapped for one before a failed rename.
/// The removal of the link may be refused (macOS, Windows: the file denies delete), but nothing
/// is unlocked through it: the lawyer's file keeps its ACL (deny-read entry included), mode and
/// attributes, byte for byte.
#[test]
fn a_hard_link_at_a_temp_name_never_changes_the_linked_file() {
    let dir = TestDir::new("leftover-hard-link");
    let lease = old_file(&dir, "Lease.docx");
    let other = old_file(&dir, "Other.docx");
    protect(&lease);
    let before = fingerprint(&lease);

    std::fs::hard_link(&lease, dir.path().join(".clauseo-9.tmp")).unwrap();
    super::sweep_temps(dir.path());
    rewrite(&other).unwrap();
    assert_eq!(fingerprint(&lease), before);

    let staged = super::stage(&other, &mut |file: &mut File| {
        file.write_all(b"newer")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    let temp = staged.temp.clone().unwrap();
    super::remove_file(&temp).unwrap();
    std::fs::hard_link(&lease, &temp).unwrap();
    super::fail_at(Some("rename"));
    let result = staged.rename();
    super::fail_at(None);
    assert!(result.is_err());
    assert_eq!(fingerprint(&lease), before);
    assert_eq!(std::fs::read(&other).unwrap(), b"new");

    unprotect(&lease);
}

/// Windows: a junction at a temp name, to a folder whose DACL denies delete: met by the sweep, and
/// swapped in for the write's own temp file before a failed rename. Neither follows it: the
/// folder keeps its DACL and attributes.
#[cfg(windows)]
#[test]
fn a_junction_at_a_temp_name_never_changes_its_target_on_windows() {
    use super::windows::{dacl_text, set_attributes, set_dacl};
    use std::os::windows::fs::MetadataExt as _;
    let dir = TestDir::new("leftover-junction");
    let shared = dir.path().join("Shared");
    super::create_dir(&shared).unwrap();
    set_dacl(&shared, "D:P(D;OICI;SD;;;WD)(A;OICI;FA;;;WD)").unwrap();
    // Hidden, on a folder.
    set_attributes(&shared, 0x12).unwrap();
    let state = |path: &Path| {
        let attributes = std::fs::symlink_metadata(path).unwrap().file_attributes();
        format!("{attributes:x} {}", dacl_text(path))
    };
    let before = state(&shared);
    let junction = |link: &Path| {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(&shared)
            .status()
            .unwrap();
        assert!(made.success(), "mklink /J");
    };

    let leftover = dir.path().join(".clauseo-5.tmp");
    junction(&leftover);
    super::sweep_temps(dir.path());
    let target = old_file(&dir, "Lease.docx");
    rewrite(&target).unwrap();
    assert_eq!(state(&shared), before);

    let staged = super::stage(&target, &mut |file: &mut File| {
        file.write_all(b"newer")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    let temp = staged.temp.clone().unwrap();
    super::remove_file(&temp).unwrap();
    junction(&temp);
    super::fail_at(Some("rename"));
    let result = staged.rename();
    super::fail_at(None);
    assert!(result.is_err());
    assert_eq!(state(&shared), before);

    for link in [&leftover, &temp] {
        if let Err(err) = super::remove_dir(link) {
            assert_eq!(err.kind(), std::io::ErrorKind::NotFound, "{err}");
        }
    }
    set_attributes(&shared, 0x10).unwrap();
    set_dacl(&shared, "D:P(A;OICI;FA;;;WD)").unwrap();
}
