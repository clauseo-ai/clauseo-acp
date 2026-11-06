//! `SystemFiles` on a real disk, under `CARGO_TARGET_TMPDIR` only: a `ui` change applied with
//! `Effective::apply_ui_patch` is saved to a settings file that loads back through
//! `Effective::load`.

use clauseo_settings::{Effective, Files as _, FontSize, Locations, SystemFiles, Theme, UiPatch};
use std::path::PathBuf;
use std::process;

#[cfg(unix)]
#[path = "disk/write.rs"]
mod write;

/// The effective settings of `locations` with no policy, as a fresh start loads them.
fn load(locations: &Locations) -> Effective {
    Effective::load(locations, &SystemFiles, &[])
}

#[test]
fn round_trip_under_target_tmpdir() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("settings-round-trip-{}", process::id()));
    let locations = Locations::under_root(&root);
    let files = SystemFiles;

    // The first write creates the data folder; the second keeps what the first wrote.
    files
        .write_atomic(locations.settings_file(), b"{\"zeta\": true}")
        .unwrap();
    let mut effective = load(&locations);
    let theme = UiPatch {
        theme: Some(Theme::Dark),
        ..UiPatch::default()
    };
    effective.apply_ui_patch(&files, &theme).unwrap();
    let size = UiPatch {
        font_size: Some(FontSize::try_from(18).unwrap()),
        ..UiPatch::default()
    };
    effective.apply_ui_patch(&files, &size).unwrap();
    assert_eq!(effective.settings().ui.font_size.get(), 18);

    let effective = load(&locations);
    assert_eq!(effective.settings().ui.theme, Theme::Dark);
    assert_eq!(effective.settings().ui.font_size.get(), 18);
    // The unknown key stayed, and is the one warning.
    assert_eq!(effective.warnings().len(), 1, "{:?}", effective.warnings());

    // No temporary file is left beside it, and a missing file reads as none.
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["settings.json"]);
    assert_eq!(files.read(&root.join("missing.json")).unwrap(), None);
}

#[test]
fn reading_a_folder_is_an_error_not_a_missing_file() {
    // Only a missing file reads as `None`; any other failure is an error.
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    assert!(SystemFiles.read(&folder).is_err());
}

/// A fresh folder for one test, under `CARGO_TARGET_TMPDIR`.
#[cfg(unix)]
fn fresh(test: &str) -> std::io::Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("settings-{test}-{}", process::id()));
    write::fresh_dir(&root)?;
    Ok(root)
}

#[cfg(unix)]
#[test]
fn saving_through_a_symlink_writes_the_linked_file_and_keeps_the_link() {
    let root = fresh("symlink").unwrap();
    let locations = Locations::under_root(&root);
    let real = root.join("dotfiles-settings.json");
    write::write(&real, b"{\"zeta\": 1}").unwrap();
    // A relative link, as a dotfiles manager makes it.
    write::symlink(
        std::path::Path::new("dotfiles-settings.json"),
        locations.settings_file(),
    )
    .unwrap();
    let dark = UiPatch {
        theme: Some(Theme::Dark),
        ..UiPatch::default()
    };
    load(&locations)
        .apply_ui_patch(&SystemFiles, &dark)
        .unwrap();

    let link = std::fs::symlink_metadata(locations.settings_file()).unwrap();
    assert!(link.file_type().is_symlink());
    let text = std::fs::read_to_string(&real).unwrap();
    assert!(
        text.contains("\"zeta\": 1") && text.contains("\"dark\""),
        "{text}"
    );
    assert_eq!(load(&locations).settings().ui.theme, Theme::Dark);
    // Only the two names: no temporary file is left in the folder.
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["dotfiles-settings.json", "settings.json"]);
}

#[cfg(unix)]
#[test]
fn saving_keeps_the_files_permissions() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = fresh("permissions").unwrap();
    let locations = Locations::under_root(&root);
    write::write(locations.settings_file(), b"{}").unwrap();
    write::set_permissions(
        locations.settings_file(),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let dark = UiPatch {
        theme: Some(Theme::Dark),
        ..UiPatch::default()
    };
    load(&locations)
        .apply_ui_patch(&SystemFiles, &dark)
        .unwrap();
    let mode = std::fs::metadata(locations.settings_file())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn a_write_syncs_its_folder_and_the_file_reads_back() {
    // The rename is synced through its folder (`files.rs`): the write still succeeds, the
    // file reads back whole, and the folder holds that file alone.
    let root = fresh("folder-sync").unwrap();
    let path = root.join("key-stored.json");
    SystemFiles
        .write_atomic(&path, b"{\"agents\": [\"claude-acp\"]}")
        .unwrap();
    SystemFiles
        .write_atomic(&path, b"{\"agents\": []}")
        .unwrap();
    assert_eq!(
        SystemFiles.read(&path).unwrap().as_deref(),
        Some(&b"{\"agents\": []}"[..])
    );
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["key-stored.json"]);
}

#[cfg(unix)]
#[test]
fn a_folder_that_will_not_sync_still_takes_the_write() {
    // The rename is in place before the folder is synced; a folder that cannot be opened to
    // sync (no read permission here) is logged, and the write is not a failed one.
    use std::os::unix::fs::PermissionsExt as _;
    let root = fresh("folder-no-sync").unwrap();
    let path = root.join("settings.json");
    write::set_permissions(&root, std::fs::Permissions::from_mode(0o300)).unwrap();
    let written = SystemFiles.write_atomic(&path, b"{}");
    write::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    written.unwrap();
    assert_eq!(
        SystemFiles.read(&path).unwrap().as_deref(),
        Some(&b"{}"[..])
    );
}

#[cfg(unix)]
#[test]
fn a_settings_path_under_a_file_is_an_error_and_creates_nothing() {
    // `file.txt` is a regular file, so the link check of `file.txt/settings.json` fails with
    // ENOTDIR (not NotFound), and the write returns that error before it creates anything.
    let root = fresh("not-a-folder").unwrap();
    write::write(&root.join("file.txt"), b"not a folder").unwrap();
    let path = root.join("file.txt").join("settings.json");
    let error = SystemFiles.write_atomic(&path, b"{}").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory, "{error}");
    assert_eq!(
        std::fs::read(root.join("file.txt")).unwrap(),
        b"not a folder"
    );
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["file.txt"]);
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_is_an_error() {
    let root = fresh("loop").unwrap();
    let (a, b) = (root.join("a.json"), root.join("b.json"));
    write::symlink(&b, &a).unwrap();
    write::symlink(&a, &b).unwrap();
    let error = SystemFiles.write_atomic(&a, b"{}").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{error}");
}

// A managed preferences file that is a symlink is read through the link, not refused.
#[cfg(unix)]
#[test]
fn a_symlinked_managed_preferences_file_is_read_through_the_link() {
    let root = fresh("managed-symlink").unwrap();
    let real = root.join("real.plist");
    write::write(
        &real,
        br#"<plist version="1.0"><dict><key>skills</key><dict/></dict></plist>"#,
    )
    .unwrap();
    let link = root.join("ai.clauseo.desktop.plist");
    write::symlink(std::path::Path::new("real.plist"), &link).unwrap();
    let names = clauseo_settings::policy::managed_prefs::file_names(&SystemFiles, &[link]).unwrap();
    assert_eq!(names, ["skills"]);
}
