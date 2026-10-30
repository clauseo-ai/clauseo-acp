use super::parse;
use crate::error::Error;
use crate::files::Files;
use crate::leaf::Leaf;
use crate::policy::{FilePolicy, PolicyRead, PolicySource};
use crate::schema::Theme;
use crate::testing::MemFiles;
use crate::warning::{Origin, Position, WarningKind};
use std::path::{Path, PathBuf};

const PATH: &str = "/etc/clauseo/policy.json";

#[test]
fn a_policy_file_has_the_settings_shape() {
    let values = parse(
        Path::new(PATH),
        b"{\n  // the firm's look\n  \"ui\": {\"theme\": \"dark\", \"density\": 1},\n}",
    )
    .unwrap();
    // The density does not type, so it is locked at its default.
    assert_eq!(
        values.keys().collect::<Vec<_>>(),
        ["ui.theme", "ui.density"]
    );
    let warning = &values.warnings()[0];
    assert_eq!(
        warning.origin,
        Origin::File {
            path: PathBuf::from(PATH),
            position: Some(Position {
                line: 3,
                column: 38
            }),
        }
    );
    assert!(matches!(&warning.kind, WarningKind::InvalidValue { key, .. } if key == "ui.density"));
    assert_eq!(
        values.into_parts().0,
        [
            Leaf::Theme(Theme::Dark),
            Leaf::Refused("ui.density".to_owned())
        ]
    );
}

#[test]
fn a_file_that_does_not_parse_is_an_error_with_its_place() {
    let error = parse(Path::new(PATH), b"{\"ui\": {}\n \"x\"}").unwrap_err();
    assert!(
        matches!(&error, Error::Parse { path, line: 2, column: 2, message }
            if path == Path::new(PATH) && message == "expected `,` or `}`"),
        "{error}"
    );
}

fn source(files: MemFiles) -> FilePolicy<MemFiles> {
    FilePolicy::new(PathBuf::from(PATH), files)
}

#[test]
fn a_file_policy_is_absent_read_or_unreadable() {
    assert!(matches!(
        source(MemFiles::default()).read(),
        PolicyRead::Absent
    ));
    let read = source(MemFiles::with(
        Path::new(PATH),
        r#"{"ui": {"theme": "dark"}}"#,
    ))
    .read();
    assert!(matches!(&read, PolicyRead::Read(values) if values.keys().eq(["ui.theme".to_owned()])));
    let broken = source(MemFiles::with(Path::new(PATH), "{")).read();
    assert!(matches!(
        broken,
        PolicyRead::Unreadable(Error::Parse { .. })
    ));
    let denied = source(MemFiles {
        broken: vec![PathBuf::from(PATH)],
        ..MemFiles::default()
    });
    assert!(matches!(
        denied.read(),
        PolicyRead::Unreadable(Error::Io { path, .. }) if path == Path::new(PATH)
    ));
    assert_eq!(denied.name(), Path::new(PATH).display().to_string());
    // FilePolicy reads through the Files it was given, not the disk.
    assert!(MemFiles::default().read(Path::new(PATH)).unwrap().is_none());
}

#[cfg(not(any(windows, target_os = "macos")))]
#[test]
fn system_policy_absent_on_clean_machine() {
    let sources = crate::system_policy_sources();
    let names: Vec<_> = sources.iter().map(|source| source.name()).collect();
    assert_eq!(names, [PATH]);
    for source in &sources {
        assert!(matches!(source.read(), PolicyRead::Absent));
    }
}
