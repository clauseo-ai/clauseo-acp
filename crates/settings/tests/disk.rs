//! `SystemFiles` on a real disk, under `CARGO_TARGET_TMPDIR` only: a settings file written with
//! `write_user_settings` loads back through `Effective::load`.

use clauseo_settings::{
    Effective, Files as _, FontSize, Locations, SystemFiles, Theme, UiPatch, write_user_settings,
};
use std::path::PathBuf;
use std::process;

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
    let theme = UiPatch {
        theme: Some(Theme::Dark),
        ..UiPatch::default()
    };
    write_user_settings(&locations, &files, &theme).unwrap();
    let size = UiPatch {
        font_size: Some(FontSize::try_from(18).unwrap()),
        ..UiPatch::default()
    };
    write_user_settings(&locations, &files, &size).unwrap();

    let effective = Effective::load(&locations, &files, &[]);
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
