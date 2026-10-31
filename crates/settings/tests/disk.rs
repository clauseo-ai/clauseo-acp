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
fn a_symlink_loop_is_an_error() {
    let root = fresh("loop").unwrap();
    let (a, b) = (root.join("a.json"), root.join("b.json"));
    write::symlink(&b, &a).unwrap();
    write::symlink(&a, &b).unwrap();
    let error = SystemFiles.write_atomic(&a, b"{}").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{error}");
}
