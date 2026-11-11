//! A matter on a volume with no file IDs (Windows' `FileIdInfo` fails with
//! `ERROR_INVALID_PARAMETER` on FAT and exFAT) through the fake disk: on a local FAT or exFAT
//! volume an object is not the data folder, so the matter's writes pass; the same failure on the
//! network or on any other file system still refuses, and an alias that opens on the data folder
//! is still caught by its identity. On the fake disk, so it runs everywhere (`C:\` on Windows).

use std::path::{Path, PathBuf};

use super::resolve::Disk;
use super::{DataFenceHealth, FenceCheck, candidates};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::tests::plain_net;

/// `place` (`/`-separated) under this system's root: `/` on Unix, `C:\` on Windows.
fn at(place: &str) -> PathBuf {
    let root = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
    place.split('/').fold(root, |path, name| path.join(name))
}

/// The data folder `d` (identity read), and the matter `x/matter` on the volume `x`, whose
/// `FileIdInfo` fails with 87, its file system named `fs_name`, remote when `remote`.
fn disk(fs_name: Option<&str>, remote: bool) -> FakeFs {
    let names: Vec<(String, Fake)> = ["d/state.sqlite", "x/matter/notes.md"]
        .iter()
        .map(|place| (at(place).display().to_string(), Fake::File))
        .collect();
    let names: Vec<(&str, Fake)> = names
        .iter()
        .map(|(name, kind)| (name.as_str(), kind.clone()))
        .collect();
    let mut fs = FakeFs::with(&names);
    fs.ids.insert(at("d"), (1, 7));
    fs.no_id_volumes
        .push((at("x"), fs_name.map(str::to_owned), remote));
    fs
}

/// A request from the matter `x/matter` naming `paths`, its own folder as a search root when
/// `search`.
fn request(health: &DataFenceHealth, data: &str, paths: &[PathBuf], search: bool) -> FenceCheck {
    FenceCheck {
        data: at(data),
        allowed: Vec::new(),
        cwd: at("x/matter"),
        home: None,
        paths: candidates(paths, None, cfg!(windows)),
        roots: if search {
            vec![at("x/matter")]
        } else {
            Vec::new()
        },
        words: Vec::new(),
        health: health.clone(),
        vars: Vec::new(),
    }
}

/// Whether the fence refuses `check` on `fs`, with no canonical step.
fn refused(check: &FenceCheck, fs: &FakeFs) -> bool {
    let net: NetEnv = plain_net(cfg!(windows));
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net: &net,
    };
    check.touches_data_on(false, &disk)
}

/// The matter's file, a new file beside it, and the matter as a search root.
fn matter_writes(health: &DataFenceHealth) -> Vec<FenceCheck> {
    let notes = at("x/matter/notes.md");
    let new = at("x/matter/new.md");
    vec![
        request(health, "d", std::slice::from_ref(&notes), false),
        request(health, "d", &[new], false),
        request(health, "d", &[notes], true),
    ]
}

#[test]
fn a_matter_on_a_local_fat_or_exfat_volume_is_not_the_data_folder() {
    for name in ["exFAT", "FAT", "FAT32", "EXFAT"] {
        let fs = disk(Some(name), false);
        let health = DataFenceHealth::default();
        for check in matter_writes(&health) {
            assert!(!refused(&check, &fs), "{name} {:?}", check.paths);
        }
        assert!(!health.names_only());
        // the data folder by its text is still refused
        let data = request(&health, "d", &[at("d/state.sqlite")], false);
        assert!(refused(&data, &fs), "{name}");
    }
}

#[test]
fn the_same_failure_on_the_network_is_refused() {
    for name in [Some("exFAT"), Some("FAT32"), Some("NTFS"), None] {
        let fs = disk(name, true);
        let health = DataFenceHealth::default();
        // a new file's own name is missing, but its folders still refuse it
        for check in matter_writes(&health) {
            assert!(refused(&check, &fs), "{name:?} {:?}", check.paths);
        }
    }
}

#[test]
fn the_same_failure_on_any_other_file_system_is_refused() {
    for name in [Some("NTFS"), Some("ReFS"), Some("vfat"), Some(""), None] {
        let fs = disk(name, false);
        let health = DataFenceHealth::default();
        for check in matter_writes(&health) {
            assert!(refused(&check, &fs), "{name:?} {:?}", check.paths);
        }
    }
}

/// A link on the volume with no file IDs that opens on the data folder (a junction followed, an
/// alias the canonical step does not give back) answers with the data folder's identity, so it
/// is still caught.
#[test]
fn an_alias_that_opens_on_the_data_folder_is_still_caught() {
    let mut fs = disk(Some("exFAT"), false);
    fs.names.insert(at("x/matter/j"), Fake::Dir);
    fs.names.insert(at("x/matter/j/state.sqlite"), Fake::File);
    fs.ids.insert(at("x/matter/j"), (1, 7));
    let health = DataFenceHealth::default();
    let alias = request(&health, "d", &[at("x/matter/j/state.sqlite")], false);
    assert!(refused(&alias, &fs));
    let mut search = request(&health, "d", &[], false);
    search.roots = vec![at("x/matter/j")];
    assert!(refused(&search, &fs));
    // the matter's other files still pass
    assert!(!refused(
        &request(&health, "d", &[at("x/matter/notes.md")], false),
        &fs
    ));
}

/// A data folder that is itself on a volume with no file IDs keeps the names-only fallback.
#[test]
fn a_data_folder_on_exfat_keeps_the_names_only_fallback() {
    let mut fs = disk(Some("exFAT"), false);
    fs.names.insert(at("x/d"), Fake::Dir);
    fs.names.insert(at("x/d/state.sqlite"), Fake::File);
    let health = DataFenceHealth::default();
    let data = request(&health, "x/d", &[at("x/d/state.sqlite")], false);
    assert!(refused(&data, &fs));
    assert!(health.names_only());
    let notes = request(&health, "x/d", &[at("x/matter/notes.md")], false);
    assert!(!refused(&notes, &fs));
}
