use super::{read, write_user_settings};
use crate::error::Error;
use crate::leaf::Leaf;
use crate::locations::Locations;
use crate::schema::{Density, FontSize, Theme};
use crate::testing::MemFiles;
use crate::ui::UiPatch;
use crate::warning::{Origin, Position, Warning, WarningKind};
use std::path::{Path, PathBuf};

fn locations() -> Locations {
    Locations::under_root(Path::new("/data"))
}

fn settings_file() -> PathBuf {
    locations().settings_file().to_owned()
}

fn read_text(text: &str) -> (Vec<Leaf>, Vec<Warning>) {
    let files = MemFiles::with(&settings_file(), text);
    let mut warnings = Vec::new();
    let leaves = read(&settings_file(), &files, &mut warnings);
    (leaves, warnings)
}

#[test]
fn jsonc_comments_and_trailing_commas() {
    let (leaves, warnings) = read_text(
        "// Clauseo settings\n{\n  /* the look */\n  \"ui\": {\n    \"theme\": \"dark\", // night\n    \
         \"density\": \"compact\",\n  },\n}\n",
    );
    assert_eq!(
        leaves,
        vec![Leaf::Theme(Theme::Dark), Leaf::Density(Density::Compact)]
    );
    assert_eq!(warnings, Vec::new());
}

#[test]
fn invalid_file_warns_and_uses_defaults() {
    let (leaves, warnings) = read_text("{\n  \"ui\": {\"theme\": \"dark\"\n");
    assert_eq!(leaves, Vec::new());
    assert_eq!(warnings.len(), 1);
    let warning = &warnings[0];
    assert_eq!(
        warning.origin,
        Origin::File {
            path: settings_file(),
            position: Some(Position { line: 3, column: 0 }),
        }
    );
    assert!(
        matches!(&warning.kind, WarningKind::Syntax { message } if message == "EOF while parsing an object"),
        "{warning}"
    );
}

#[test]
fn unknown_key_is_a_warning() {
    let (leaves, warnings) = read_text("{\"ui\": {\"colour\": \"teal\", \"theme\": \"light\"}}");
    assert_eq!(leaves, vec![Leaf::Theme(Theme::Light)]);
    assert_eq!(
        warnings,
        vec![Warning {
            origin: Origin::File {
                path: settings_file(),
                position: Some(Position {
                    line: 1,
                    column: 19
                }),
            },
            kind: WarningKind::UnknownKey {
                key: "ui.colour".to_owned()
            },
        }]
    );
}

#[test]
fn a_missing_file_holds_nothing_and_says_nothing() {
    let mut warnings = Vec::new();
    assert_eq!(
        read(&settings_file(), &MemFiles::default(), &mut warnings),
        Vec::new()
    );
    assert_eq!(warnings, Vec::new());
}

#[test]
fn an_unreadable_file_warns_about_the_whole_file() {
    let files = MemFiles {
        broken: vec![settings_file()],
        ..MemFiles::default()
    };
    let mut warnings = Vec::new();
    assert_eq!(read(&settings_file(), &files, &mut warnings), Vec::new());
    assert_eq!(
        warnings,
        vec![Warning {
            origin: Origin::File {
                path: settings_file(),
                position: None,
            },
            kind: WarningKind::Unreadable {
                message: "denied".to_owned()
            },
        }]
    );
}

fn dark() -> UiPatch {
    UiPatch {
        theme: Some(Theme::Dark),
        font_size: Some(FontSize::try_from(16).unwrap()),
        ..UiPatch::default()
    }
}

#[test]
fn writing_without_a_file_creates_one_with_the_patch() {
    let files = MemFiles::default();
    write_user_settings(&locations(), &files, &dark()).unwrap();
    assert_eq!(
        files.text(&settings_file()).unwrap(),
        "{\n  \"ui\": {\n    \"theme\": \"dark\",\n    \"font_size\": 16\n  }\n}\n"
    );
}

#[test]
fn writing_keeps_every_other_key_in_its_order() {
    let files = MemFiles::with(
        &settings_file(),
        "\u{feff}{\"zeta\": 1, // M2 key\n\"ui\": {\"language\": \"fr\", \"theme\": \"light\",}, \
         \"agents\": {},}",
    );
    write_user_settings(&locations(), &files, &dark()).unwrap();
    assert_eq!(
        files.text(&settings_file()).unwrap(),
        "{\n  \"zeta\": 1,\n  \"ui\": {\n    \"language\": \"fr\",\n    \"theme\": \"dark\",\n    \
         \"font_size\": 16\n  },\n  \"agents\": {}\n}\n"
    );
}

#[test]
fn writing_replaces_a_ui_that_is_no_object() {
    let files = MemFiles::with(&settings_file(), "{\"ui\": \"big\"}");
    write_user_settings(&locations(), &files, &UiPatch::default()).unwrap();
    assert_eq!(
        files.text(&settings_file()).unwrap(),
        "{\n  \"ui\": {}\n}\n"
    );
}

#[test]
fn writing_leaves_a_broken_file_alone() {
    let files = MemFiles::with(&settings_file(), "{\"ui\": ");
    let error = write_user_settings(&locations(), &files, &dark()).unwrap_err();
    assert!(
        matches!(&error, Error::Parse { path, line: 1, column: 7, message }
            if *path == settings_file() && message == "EOF while parsing a value"),
        "{error}"
    );
    assert_eq!(files.text(&settings_file()).unwrap(), "{\"ui\": ");
}

#[test]
fn writing_reports_read_and_write_failures() {
    let unreadable = MemFiles {
        broken: vec![settings_file()],
        ..MemFiles::default()
    };
    let error = write_user_settings(&locations(), &unreadable, &dark()).unwrap_err();
    assert!(matches!(&error, Error::Io { path, .. } if *path == settings_file()));

    let full = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    let error = write_user_settings(&locations(), &full, &dark()).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{}: disk full", settings_file().display())
    );
}
