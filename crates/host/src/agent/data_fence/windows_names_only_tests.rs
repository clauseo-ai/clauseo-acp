//! The data folder's identity forced unreadable on a real Windows volume (Windows only): the
//! real disk with one seam, an `Fs` whose identity read fails for the data folder. A junction to
//! the data folder (the walk follows it) and its 8.3 short name (the canonical step gives the
//! long form), as a path and as a shell word, are still refused by name; an ordinary matter
//! passes; the fence's health says names only, and clears once the identity reads.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::resolve::Disk;
use super::tests::{check, layout, text};
use super::{DataFenceHealth, FenceCheck};
use crate::network_paths::resolve::{Entry, Fs, RealFs};
use crate::network_paths::{FOLD_CASE, NetEnv, short_path};

/// The real disk, but `unreadable`'s identity fails (access denied) while it is set.
struct DataUnreadable {
    unreadable: Option<PathBuf>,
}

impl Fs for DataUnreadable {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        RealFs.entry(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        RealFs.read_link(path)
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        if self.unreadable.as_deref() == Some(path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        RealFs.identity(path)
    }
}

/// Whether the fence refuses `check` on the real disk with `fs`'s seam.
fn refused(check: &FenceCheck, fs: &DataUnreadable) -> bool {
    let net = NetEnv::current();
    let disk = Disk {
        fs,
        ..Disk::real(&net)
    };
    check.touches_data_on(FOLD_CASE, &disk)
}

fn with(health: &DataFenceHealth, mut check: FenceCheck) -> FenceCheck {
    check.health = health.clone();
    check
}

#[test]
fn with_the_data_folders_identity_unreadable_a_junction_and_a_short_name_are_still_refused() {
    let dir = layout("win-names-only");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let data = root.join("data");
    let junction = root.join("matter").join("j");
    clauseo_sandbox::windows::test_junction::junction(&junction, &data).unwrap();
    let mut fs = DataUnreadable {
        unreadable: Some(data.clone()),
    };
    let health = DataFenceHealth::default();
    let path = |path: &str| with(&health, check(&root, &[path], None));
    let command = |command: String| {
        with(
            &health,
            check(&root, &[], Some(&json!({ "command": command }))),
        )
    };

    for place in [
        r"matter\sub\a.txt",
        r"matter\sub\new.docx",
        r"outside\x.txt",
    ] {
        assert!(!refused(&path(&text(&root.join(place))), &fs), "{place}");
    }
    assert!(health.names_only());
    let through = text(&junction.join("state.sqlite"));
    assert!(refused(&path(&through), &fs));
    assert!(refused(&command(format!("type {through}")), &fs));
    match short_path(&data).filter(|short| *short != data) {
        Some(short) => {
            let alias = text(&short.join("state.sqlite"));
            assert!(refused(&path(&alias), &fs), "{alias}");
            assert!(refused(&command(format!("type {alias}")), &fs), "{alias}");
        }
        None => eprintln!("skipped the 8.3 case: the volume gave no short name"),
    }
    assert!(health.names_only());

    // The identity reads again: the next check clears the flag.
    fs.unreadable = None;
    assert!(!refused(&path(&text(&root.join(r"matter\sub\a.txt"))), &fs));
    assert!(!health.names_only());
    assert!(refused(&path(&through), &fs));
}
