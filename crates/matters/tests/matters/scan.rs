//! `scan`: counts, the largest files, cancel, and the refused folders.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use clauseo_matters::{Error, LargeFile, Limits, Progress, Refusal, ScanReport, Scanned, scan};
use clauseo_protocol::MatterPath;

use crate::support::{self, MarkedProbe, TestDir};

fn report(folder: &Path, refused: &[PathBuf], settings: &clauseo_matters::Settings) -> ScanReport {
    let cancel = AtomicBool::new(false);
    match scan(folder, refused, settings, &mut |_| {}, &cancel).unwrap() {
        Scanned::Done(report) => report,
        Scanned::Cancelled => panic!("cancelled"),
    }
}

fn refusal(folder: &Path, refused: &[PathBuf]) -> Refusal {
    let dir = TestDir::new("unused-root");
    let settings = support::settings(dir.path(), Arc::default());
    let cancel = AtomicBool::new(false);
    match scan(folder, refused, &settings, &mut |_| {}, &cancel) {
        Err(Error::FolderRefused { reason, .. }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn large(path: &str, bytes: u64) -> LargeFile {
    LargeFile {
        path: MatterPath::new(path).unwrap(),
        bytes,
    }
}

#[test]
fn import_scan_counts_files_bytes_cloud_only() {
    let dir = TestDir::new("scan-counts");
    let folder = dir.path().join("matter");
    // Twelve files of 1 to 12 bytes; over a cap of 10 the last two are over-cap.
    for n in 1..=12_u8 {
        support::write(
            folder.join(format!("docs/f{n:02}.txt")),
            vec![b'x'; usize::from(n)],
        );
    }
    support::write(folder.join("tie-b.txt"), [0; 9]);
    support::write(folder.join("tie-a.txt"), [0; 9]);
    support::write(folder.join("cloud.docx"), [0; 100]);
    for excluded in [
        "~$f01.txt",
        "x.tmp",
        "Thumbs.db",
        ".DS_Store",
        ".git/HEAD",
        ".clauseo/m",
    ] {
        support::write(folder.join(excluded), [0; 50]);
    }
    let probe = Arc::new(MarkedProbe::default());
    probe.mark(folder.join("cloud.docx"));
    let mut settings = support::settings(dir.path().join("root"), Arc::clone(&probe));
    settings.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 50,
    };

    let mut seen = Vec::new();
    let cancel = AtomicBool::new(false);
    let Scanned::Done(scanned) = scan(
        &folder,
        &[],
        &settings,
        &mut |progress| seen.push(progress),
        &cancel,
    )
    .unwrap() else {
        panic!("cancelled");
    };
    // f01..f10 and the two ties are versioned: 55 + 18 bytes.
    assert_eq!(scanned.files, 12);
    assert_eq!(scanned.bytes, 73);
    assert_eq!(scanned.cloud_only, 1);
    assert_eq!(scanned.over_cap, 2);
    assert_eq!(
        scanned.largest,
        [
            large("docs/f12.txt", 12),
            large("docs/f11.txt", 11),
            large("docs/f10.txt", 10),
            large("docs/f09.txt", 9),
            large("tie-a.txt", 9),
            large("tie-b.txt", 9),
            large("docs/f08.txt", 8),
            large("docs/f07.txt", 7),
            large("docs/f06.txt", 6),
            large("docs/f05.txt", 5),
        ]
    );
    assert_eq!(
        scanned.warnings,
        [clauseo_matters::ImportWarning::OverThreshold { bytes: 73 }]
    );
    // One progress call per file seen (cloud-only included), counting up to all of them.
    assert_eq!(seen.len(), 15);
    assert_eq!(
        seen.last(),
        Some(&Progress {
            files: 15,
            bytes: 73 + 23 + 100
        })
    );

    // Unmarked, the placeholder is a file again (over the cap); a removed file is gone; an
    // mtime alone changes nothing.
    probe.unmark(folder.join("cloud.docx"));
    support::remove_file(folder.join("docs/f01.txt"));
    support::set_mtime(
        folder.join("docs/f02.txt"),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
    );
    let again = report(&folder, &[], &settings);
    assert_eq!((again.files, again.bytes), (11, 72));
    assert_eq!((again.cloud_only, again.over_cap), (0, 3));
    assert_eq!(again.largest.first(), Some(&large("cloud.docx", 100)));
}

#[test]
fn import_scan_cancelled_before_it_starts() {
    let dir = TestDir::new("scan-cancel");
    support::write(dir.path().join("matter/a.txt"), "a");
    let settings = support::settings(dir.path().join("root"), Arc::default());
    let cancel = AtomicBool::new(true);
    let scanned = scan(
        &dir.path().join("matter"),
        &[],
        &settings,
        &mut |_| {},
        &cancel,
    );
    assert_eq!(scanned.unwrap(), Scanned::Cancelled);
}

#[test]
fn import_refuses_listed_folders() {
    let dir = TestDir::new("refuse-listed");
    let listed = dir.path().join("system");
    support::create_dir_all(&listed);
    let settings = support::settings(dir.path().join("root"), Arc::default());
    let cancel = AtomicBool::new(false);
    let err = scan(
        &listed,
        std::slice::from_ref(&listed),
        &settings,
        &mut |_| {},
        &cancel,
    )
    .unwrap_err();
    assert_eq!(err.code().as_str(), "matter.folder_refused");
    assert!(
        matches!(&err, Error::FolderRefused { reason: Refusal::Listed(entry), .. }
            if *entry == listed),
        "{err:?}"
    );
}

#[test]
fn import_refuses_parent_of_listed_folder() {
    let dir = TestDir::new("refuse-parent");
    let parent = dir.path().join("parent");
    let listed = parent.join("a/system");
    support::create_dir_all(&listed);
    assert_eq!(
        refusal(&parent, std::slice::from_ref(&listed)),
        Refusal::Listed(listed)
    );
}

#[test]
fn import_allows_folder_inside_listed_one() {
    let dir = TestDir::new("refuse-inside");
    let listed = dir.path().join("home");
    let inside = listed.join("Matters/Smith");
    support::write(inside.join("a.txt"), "a");
    let settings = support::settings(dir.path().join("root"), Arc::default());
    // A sibling whose name extends the listed one's is not inside it either.
    let sibling = dir.path().join("home2");
    support::create_dir_all(&sibling);
    assert_eq!(
        report(&inside, std::slice::from_ref(&listed), &settings).files,
        1
    );
    assert_eq!(report(&sibling, &[listed], &settings).files, 0);
}

#[cfg(unix)]
#[test]
fn import_refuses_symlinked_spelling() {
    let dir = TestDir::new("refuse-symlink");
    let listed = dir.path().join("system");
    support::create_dir_all(&listed);
    // A symlink to a listed folder is that folder.
    let link = dir.path().join("link-to-system");
    support::symlink(&listed, &link);
    assert_eq!(
        refusal(&link, std::slice::from_ref(&listed)),
        Refusal::Listed(listed.clone())
    );
    // A listed entry that is itself a symlink names the folder it points to.
    let listed_link = dir.path().join("listed-link");
    support::symlink(&listed, &listed_link);
    assert_eq!(
        refusal(&listed, std::slice::from_ref(&listed_link)),
        Refusal::Listed(listed_link)
    );
}

#[test]
fn import_refuses_dot_ssh() {
    let dir = TestDir::new("refuse-ssh");
    let home = dir.path().join("home");
    let ssh = home.join(".ssh");
    support::write(ssh.join("id_ed25519"), "not a key");
    let list = [ssh.clone()];
    assert_eq!(refusal(&home, &list), Refusal::Listed(ssh.clone()));
    assert_eq!(refusal(&ssh, &list), Refusal::Listed(ssh));
}

#[cfg(target_os = "macos")]
#[test]
fn import_refuses_firmlink_spelling_of_home() {
    let dir = TestDir::new("refuse-firmlink");
    let home = dir.path().join("home");
    support::create_dir_all(&home);
    let canonical = dunce::canonicalize(&home).unwrap();
    let firmlinked = Path::new("/System/Volumes/Data").join(canonical.strip_prefix("/").unwrap());
    assert!(firmlinked.is_dir(), "{} should exist", firmlinked.display());
    assert_eq!(
        refusal(&firmlinked, std::slice::from_ref(&home)),
        Refusal::Listed(home)
    );
}
