//! A rewrite of a lawyer's file ([`super::stage`], then [`super::Staged::rename`]) keeps the old
//! file's metadata: its mode (Unix), a user extended attribute (Linux), Finder tags, Finder info
//! and the quarantine flag (macOS), the hidden attribute and an alternate data stream (Windows).
//! Clauseo's own files ([`super::replace`]) keep nothing.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::test_dir::TestDir;

/// `name` in a fresh test folder, holding "old".
fn old_file(dir: &TestDir, name: &str) -> PathBuf {
    let target = dir.path().join(name);
    super::create_new(&target)
        .unwrap()
        .write_all(b"old")
        .unwrap();
    target
}

/// Rewrites `target` with "new" as a write-back does; the attributes lost.
fn rewrite(target: &Path) -> u32 {
    let staged = super::stage(target, &mut |file: &mut File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    let lost = staged.rename().unwrap().lost.count;
    assert_eq!(std::fs::read(target).unwrap(), b"new");
    lost
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    super::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_rewrite_keeps_the_mode() {
    let dir = TestDir::new("keep-mode");
    let target = old_file(&dir, "Lease.docx");
    for kept in [0o640, 0o600, 0o755] {
        set_mode(&target, kept);
        assert_eq!(rewrite(&target), 0);
        assert_eq!(mode(&target), kept, "{kept:o}");
    }
}

/// Setuid, setgid and sticky are never copied onto a rewritten file.
#[cfg(unix)]
#[test]
fn a_rewrite_keeps_the_permission_bits_alone() {
    let dir = TestDir::new("keep-mode-bits");
    let target = old_file(&dir, "Lease.docx");
    set_mode(&target, 0o1640);
    assert_eq!(rewrite(&target), 0);
    assert_eq!(mode(&target), 0o640);
}

/// A new file (or a restore of a deleted one: checkpoints record no metadata) gets the defaults,
/// and that is no loss.
#[cfg(unix)]
#[test]
fn a_new_file_gets_the_defaults() {
    let dir = TestDir::new("keep-new");
    let target = dir.path().join("New.docx");
    assert_eq!(rewrite(&target), 0);
    let made = old_file(&dir, "Made.docx");
    assert_eq!(mode(&target), mode(&made));
}

/// Clauseo's own files keep nothing of the old one: the defaults' permissions.
#[cfg(unix)]
#[test]
fn clauseos_own_files_keep_the_defaults() {
    let dir = TestDir::new("keep-own");
    let target = old_file(&dir, "index");
    let default = mode(&target);
    set_mode(&target, 0o700);
    super::replace(&target, &mut |file: &mut File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    assert_eq!(mode(&target), default);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_xattr(path: &Path, name: &str, value: &[u8]) -> rustix::io::Result<()> {
    rustix::fs::setxattr(path, name, value, rustix::fs::XattrFlags::empty())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn xattr(path: &Path, name: &str) -> Vec<u8> {
    let size = rustix::fs::getxattr_query(path, name).unwrap();
    let mut value = vec![0; size];
    let len = rustix::fs::getxattr(path, name, value.as_mut_slice()).unwrap();
    value.truncate(len);
    value
}

/// Linux: a `user.` attribute is kept. Skipped (passes, with a note) where the test folder's
/// file system takes no user attributes.
#[cfg(target_os = "linux")]
#[test]
fn a_rewrite_keeps_a_user_xattr_on_linux() {
    use rustix::io::Errno;
    let dir = TestDir::new("keep-xattr");
    let target = old_file(&dir, "Lease.docx");
    match set_xattr(&target, "user.clauseo.test", b"kept") {
        Ok(()) => {}
        Err(err) if err == Errno::NOTSUP || err == Errno::OPNOTSUPP => {
            eprintln!("skipped: the test folder's file system takes no user xattrs");
            return;
        }
        Err(err) => panic!("{err}"),
    }
    assert_eq!(rewrite(&target), 0);
    assert_eq!(xattr(&target, "user.clauseo.test"), b"kept");
}

/// macOS: Finder tags, Finder info (a red label) and the quarantine flag are kept.
#[cfg(target_os = "macos")]
#[test]
fn a_rewrite_keeps_finder_tags_on_macos() {
    const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";
    // Finder stores a binary plist; the copy is of bytes, so any value tests it.
    const TAG_PLIST: &[u8] = b"bplist00 Red\n6";
    let mut finder_info = [0_u8; 32];
    finder_info[9] = 0x0c;
    let quarantine = b"0081;6560c1a0;Safari;";
    let dir = TestDir::new("keep-finder");
    let target = old_file(&dir, "Lease.docx");
    set_xattr(&target, TAGS, TAG_PLIST).unwrap();
    set_xattr(&target, "com.apple.FinderInfo", &finder_info).unwrap();
    set_xattr(&target, "com.apple.quarantine", quarantine).unwrap();
    assert_eq!(rewrite(&target), 0);
    assert_eq!(xattr(&target, TAGS), TAG_PLIST);
    assert_eq!(xattr(&target, "com.apple.FinderInfo"), finder_info);
    assert_eq!(xattr(&target, "com.apple.quarantine"), quarantine);
}

/// macOS: the rewritten file keeps the original's birth time (Finder's "Created").
#[cfg(target_os = "macos")]
#[test]
fn a_rewrite_keeps_the_birth_time_on_macos() {
    use std::os::macos::fs::MetadataExt as _;
    let dir = TestDir::new("keep-birth-time");
    let target = dir.path().join("Lease.docx");
    let mut file = super::create_new(&target).unwrap();
    file.write_all(b"old").unwrap();
    super::macos::set_birth_time(&file, 1_600_000_000, 123_000_000).unwrap();
    drop(file);
    assert_eq!(rewrite(&target), 0);
    let metadata = std::fs::metadata(&target).unwrap();
    assert_eq!(
        (metadata.st_birthtime(), metadata.st_birthtime_nsec()),
        (1_600_000_000, 123_000_000)
    );
}

/// macOS: an original without the quarantine flag gives a rewrite without one.
#[cfg(target_os = "macos")]
#[test]
fn a_rewrite_of_an_unquarantined_file_stays_unquarantined_on_macos() {
    let dir = TestDir::new("keep-unquarantined");
    let target = old_file(&dir, "Lease.docx");
    let none = rustix::fs::getxattr_query(&target, "com.apple.quarantine");
    assert_eq!(none.unwrap_err(), rustix::io::Errno::NOATTR);
    assert_eq!(rewrite(&target), 0);
    let none = rustix::fs::getxattr_query(&target, "com.apple.quarantine");
    assert_eq!(none.unwrap_err(), rustix::io::Errno::NOATTR);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn only_the_systems_own_attributes_are_skipped() {
    use super::keep::system_managed;
    for name in [
        "com.apple.macl",
        "com.apple.provenance",
        "com.apple.system.Security",
    ] {
        assert!(system_managed(name.as_bytes()), "{name}");
    }
    for name in [
        "com.apple.metadata:_kMDItemUserTags",
        "com.apple.FinderInfo",
        "com.apple.quarantine",
        "user.xdg.tags",
        "system.posix_acl_access",
    ] {
        assert!(!system_managed(name.as_bytes()), "{name}");
    }
}

/// Windows: `ReplaceFileW` keeps the hidden attribute, the `Zone.Identifier` stream (the mark
/// of a downloaded file) and the creation time, which a rename over the file drops.
#[cfg(windows)]
#[test]
fn a_rewrite_keeps_hidden_and_the_zone_stream_on_windows() {
    keeps_hidden_zone_and_created("keep-windows", "Lease.docx");
}

/// Windows: names of 240 and 255 units, too long for `<name>.clauseo-<n>.orig.tmp`, keep the
/// same through two rewrites: their backup name is the short hashed one.
#[cfg(windows)]
#[test]
fn a_long_name_keeps_hidden_the_zone_stream_and_the_creation_time_on_windows() {
    for len in [240, 255] {
        let name = format!("{}.docx", "a".repeat(len - 5));
        keeps_hidden_zone_and_created("keep-windows-long", &name);
    }
}

/// Gives the file `name` a creation time in 2020, the hidden attribute and a `Zone.Identifier`
/// stream with `ZoneId=3`, rewrites it twice, and checks each time that all three are kept,
/// nothing is counted lost and the folder holds only the file.
#[cfg(windows)]
fn keeps_hidden_zone_and_created(dir_name: &str, name: &str) {
    use std::os::windows::fs::{FileTimesExt as _, MetadataExt as _};
    use std::time::{Duration, SystemTime};
    /// `FILE_ATTRIBUTE_HIDDEN` and `ARCHIVE`.
    const HIDDEN: u32 = 0x2;
    const ARCHIVE: u32 = 0x20;
    let zone = b"[ZoneTransfer]\r\nZoneId=3\r\n";
    let created = SystemTime::UNIX_EPOCH + Duration::from_hours(438_288);
    let dir = TestDir::new(dir_name);
    let target = old_file(&dir, name);
    let times = std::fs::FileTimes::new().set_created(created);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&target)
        .unwrap();
    file.set_times(times).unwrap();
    drop(file);
    let stream = dir.path().join(format!("{name}:Zone.Identifier"));
    super::create_new(&stream).unwrap().write_all(zone).unwrap();
    super::windows::set_attributes(&target, HIDDEN | ARCHIVE).unwrap();
    for round in 1..=2 {
        assert_eq!(rewrite(&target), 0, "{name}: round {round}");
        let metadata = std::fs::metadata(&target).unwrap();
        let attributes = metadata.file_attributes();
        assert_eq!(
            attributes & HIDDEN,
            HIDDEN,
            "round {round}: {attributes:#x}"
        );
        assert_eq!(metadata.created().unwrap(), created, "round {round}");
        assert_eq!(std::fs::read(&stream).unwrap(), zone, "round {round}");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(name)], "round {round}");
    }
}

/// Windows: a new file goes in by the plain rename, with nothing lost.
#[cfg(windows)]
#[test]
fn a_new_file_is_renamed_into_place_on_windows() {
    let dir = TestDir::new("keep-windows-new");
    assert_eq!(rewrite(&dir.path().join("New.docx")), 0);
}

/// Linux: a read-only original (`0o444`) keeps its user attribute too: the mode is set after
/// the attributes, which need a writable file. Skipped like the test above.
#[cfg(target_os = "linux")]
#[test]
fn a_read_only_original_keeps_a_user_xattr_on_linux() {
    use rustix::io::Errno;
    let dir = TestDir::new("keep-xattr-ro");
    let target = old_file(&dir, "Lease.docx");
    match set_xattr(&target, "user.clauseo.test", b"kept") {
        Ok(()) => {}
        Err(err) if err == Errno::NOTSUP || err == Errno::OPNOTSUPP => {
            eprintln!("skipped: the test folder's file system takes no user xattrs");
            return;
        }
        Err(err) => panic!("{err}"),
    }
    set_mode(&target, 0o444);
    assert_eq!(rewrite(&target), 0);
    assert_eq!(xattr(&target, "user.clauseo.test"), b"kept");
    assert_eq!(mode(&target), 0o444);
}

/// macOS: a read-only original (`0o444`) keeps its Finder tag and quarantine flag.
#[cfg(target_os = "macos")]
#[test]
fn a_read_only_original_keeps_its_finder_tag_on_macos() {
    const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";
    let dir = TestDir::new("keep-finder-ro");
    let target = old_file(&dir, "Lease.docx");
    set_xattr(&target, TAGS, b"bplist00 Red\n6").unwrap();
    set_xattr(&target, "com.apple.quarantine", b"0081;6560c1a0;Safari;").unwrap();
    set_mode(&target, 0o444);
    assert_eq!(rewrite(&target), 0);
    assert_eq!(xattr(&target, TAGS), b"bplist00 Red\n6");
    assert_eq!(
        xattr(&target, "com.apple.quarantine"),
        b"0081;6560c1a0;Safari;"
    );
    assert_eq!(mode(&target), 0o444);
}

/// Windows: a target with the read-only attribute stays refused, as a rename refuses it; the old
/// file is untouched and the temp file goes.
#[cfg(windows)]
#[test]
fn a_read_only_target_stays_refused_on_windows() {
    let dir = TestDir::new("keep-windows-ro");
    let target = old_file(&dir, "Lease.docx");
    let writable = std::fs::metadata(&target).unwrap().permissions();
    let mut permissions = writable.clone();
    permissions.set_readonly(true);
    super::set_permissions(&target, permissions).unwrap();
    let staged = super::stage(&target, &mut |file: &mut File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    let failed = staged.rename().unwrap_err();
    assert!(!failed.changed);
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(left.len(), 1);
    super::set_permissions(&target, writable).unwrap();
}
