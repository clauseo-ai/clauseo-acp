//! `Effective::apply_ui_patch`: the lock check, the write, and memory left alone when either
//! fails. The fakes and policy sources are the ones in `crate::testing`.

use super::Effective;
use crate::error::Error;
use crate::schema::{Density, FontSize, Language, Theme, UiSettings};
use crate::testing::{MemFiles, locations, policy, user};
use crate::ui::UiPatch;
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    locations().settings_file().to_owned()
}

/// The settings file's text, or `None` when there is no file.
fn file_text(files: &MemFiles) -> Option<String> {
    files.text(&settings_path())
}

/// A patch that sets the theme and the density.
fn dark_and_compact() -> UiPatch {
    UiPatch {
        theme: Some(Theme::Dark),
        density: Some(Density::Compact),
        ..UiPatch::default()
    }
}

#[test]
fn apply_ui_patch_writes_then_sets_memory_and_a_fresh_load_agrees() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .apply_ui_patch(&files, &dark_and_compact())
        .unwrap();
    assert_eq!(effective.settings().ui.theme, Theme::Dark);
    assert_eq!(effective.settings().ui.density, Density::Compact);
    assert_eq!(
        file_text(&files).unwrap(),
        "{\n  \"ui\": {\n    \"theme\": \"dark\",\n    \"density\": \"compact\"\n  }\n}\n"
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().ui, effective.settings().ui);
    assert_eq!(fresh.warnings(), []);
}

#[test]
fn a_patch_naming_a_locked_key_is_refused_whole_and_writes_nothing() {
    let files = user(r#"{"ui": {"density": "comfortable"}}"#);
    let firm = policy("firm", r#"{"ui": {"theme": "light"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let before = effective.settings().clone();
    let original = file_text(&files);
    // A mixed patch: the locked theme comes first, the unlocked density must not be written.
    let error = effective
        .apply_ui_patch(&files, &dark_and_compact())
        .unwrap_err();
    assert!(
        matches!(&error, Error::Locked { key } if key == "ui.theme"),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "ui.theme is set by policy and cannot be changed"
    );
    assert_eq!(file_text(&files), original);
    assert_eq!(effective.settings(), &before);
}

#[test]
fn a_patch_equal_to_the_locked_value_is_refused_too() {
    let files = MemFiles::default();
    let firm = policy("firm", r#"{"ui": {"theme": "light"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let same = UiPatch {
        theme: Some(Theme::Light),
        ..UiPatch::default()
    };
    let error = effective.apply_ui_patch(&files, &same).unwrap_err();
    assert!(
        matches!(&error, Error::Locked { key } if key == "ui.theme"),
        "{error}"
    );
    assert_eq!(file_text(&files), None);
}

#[test]
fn each_locked_key_is_reported_by_its_own_name() {
    let font = || UiPatch {
        font_size: Some(FontSize::try_from(16).unwrap()),
        ..UiPatch::default()
    };
    let cases = [
        (
            r#"{"ui": {"theme": "light"}}"#,
            UiPatch {
                theme: Some(Theme::Dark),
                ..UiPatch::default()
            },
            "ui.theme",
        ),
        (r#"{"ui": {"font_size": 18}}"#, font(), "ui.font_size"),
        // A value that does not type still locks its key (fail closed).
        (r#"{"ui": {"font_size": "big"}}"#, font(), "ui.font_size"),
        (
            r#"{"ui": {"density": "compact"}}"#,
            UiPatch {
                density: Some(Density::Comfortable),
                ..UiPatch::default()
            },
            "ui.density",
        ),
        (
            r#"{"ui": {"language": "fr"}}"#,
            UiPatch {
                language: Some(Language::try_from("en".to_owned()).unwrap()),
                ..UiPatch::default()
            },
            "ui.language",
        ),
    ];
    for (firm_text, patch, key) in cases {
        let files = MemFiles::default();
        let firm = policy("firm", firm_text);
        let mut effective = Effective::load(&locations(), &files, &[&firm]);
        let error = effective.apply_ui_patch(&files, &patch).unwrap_err();
        assert!(
            matches!(&error, Error::Locked { key: named } if named == key),
            "{firm_text}: {error}"
        );
        assert_eq!(file_text(&files), None, "{firm_text}");
    }
}

#[test]
fn the_first_locked_key_in_field_order_is_named() {
    let firm = policy(
        "firm",
        r#"{"ui": {"language": "fr", "density": "compact"}}"#,
    );
    let mut effective = Effective::load(&locations(), &MemFiles::default(), &[&firm]);
    let patch = UiPatch {
        language: Some(Language::try_from("en".to_owned()).unwrap()),
        density: Some(Density::Comfortable),
        ..UiPatch::default()
    };
    let error = effective
        .apply_ui_patch(&MemFiles::default(), &patch)
        .unwrap_err();
    assert!(
        matches!(&error, Error::Locked { key } if key == "ui.density"),
        "{error}"
    );
}

#[test]
fn a_failed_write_changes_nothing_in_memory() {
    let unreadable = MemFiles {
        broken: vec![settings_path()],
        ..MemFiles::default()
    };
    let full = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    for files in [unreadable, full] {
        let mut effective = Effective::load(&locations(), &files, &[]);
        let before = effective.settings().clone();
        let error = effective
            .apply_ui_patch(&files, &dark_and_compact())
            .unwrap_err();
        assert!(
            matches!(&error, Error::Io { path, .. } if *path == settings_path()),
            "{error}"
        );
        assert_eq!(effective.settings(), &before);
        assert_eq!(file_text(&files), None);
    }
}

#[test]
fn a_file_that_does_not_parse_is_refused_and_memory_is_unchanged() {
    let text = "{\"ui\": ";
    let files = user(text);
    let mut effective = Effective::load(&locations(), &files, &[]);
    let before = effective.settings().clone();
    let error = effective
        .apply_ui_patch(&files, &dark_and_compact())
        .unwrap_err();
    assert!(
        matches!(&error, Error::Parse { path, line: 1, column: 7, .. } if *path == settings_path()),
        "{error}"
    );
    assert_eq!(file_text(&files).as_deref(), Some(text));
    assert_eq!(effective.settings(), &before);
}

#[test]
fn an_empty_patch_leaves_a_commented_file_byte_identical() {
    let text = "// my settings\n{\n  // the look\n  \"ui\": {\"theme\": \"light\"}\n}\n";
    let files = user(text);
    let mut effective = Effective::load(&locations(), &files, &[]);
    let before = effective.settings().clone();
    effective
        .apply_ui_patch(&files, &UiPatch::default())
        .unwrap();
    assert_eq!(file_text(&files).as_deref(), Some(text));
    assert_eq!(effective.settings(), &before);
    let no_file = MemFiles::default();
    effective
        .apply_ui_patch(&no_file, &UiPatch::default())
        .unwrap();
    assert_eq!(file_text(&no_file), None);
}

#[test]
fn an_all_fields_patch_reaches_memory_and_a_fresh_load_alike() {
    // Every field set to a value that is not its default, so a field the write or the apply
    // drops shows as a default.
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    let patch = UiPatch {
        theme: Some(Theme::Dark),
        font_size: Some(FontSize::try_from(18).unwrap()),
        density: Some(Density::Compact),
        language: Some(Language::try_from("fr".to_owned()).unwrap()),
    };
    effective.apply_ui_patch(&files, &patch).unwrap();
    let expected = UiSettings {
        theme: Theme::Dark,
        font_size: FontSize::try_from(18).unwrap(),
        density: Density::Compact,
        language: Language::try_from("fr".to_owned()).unwrap(),
    };
    assert_eq!(effective.settings().ui, expected);
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().ui, expected);
    assert_eq!(fresh.warnings(), []);
}

#[test]
fn a_repeated_key_reads_the_same_after_a_save_of_another_key() {
    // The last value of `ui.theme` does not type. A save of `density` rewrites the file through
    // a map, which keeps that last value, so the theme must still read as the default.
    let files = user(r#"{"ui": {"theme": "dark", "theme": "purple"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().ui.theme, Theme::System);
    let density = UiPatch {
        density: Some(Density::Compact),
        ..UiPatch::default()
    };
    effective.apply_ui_patch(&files, &density).unwrap();
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().ui.theme, Theme::System);
    assert_eq!(fresh.settings().ui.density, Density::Compact);
    assert_eq!(fresh.warnings().len(), 1, "{:?}", fresh.warnings());
}
