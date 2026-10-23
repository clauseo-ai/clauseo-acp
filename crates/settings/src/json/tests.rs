use super::{Syntax, blank_trailing_commas, read};
use crate::leaf::Raw;
use crate::warning::{Origin, Position, Warning, WarningKind};
use serde_json::json;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

const PATH: &str = "/data/settings.json";

fn at(line: usize, column: usize) -> Origin {
    Origin::File {
        path: PathBuf::from(PATH),
        position: Some(Position { line, column }),
    }
}

fn raw(key: &str, value: serde_json::Value, line: usize, column: usize) -> Raw {
    Raw {
        key: key.to_owned(),
        value,
        origin: at(line, column),
    }
}

#[test]
fn values_come_in_file_order_with_dotted_keys_and_their_places() {
    let text = "{\n  // comment\n  \"ui\": { \"theme\": \"dark\",\n    \"font_size\": 16, },\n  \
                \"agents\": { \"a\": { \"command\": \"/x\" } },\n}\n";
    let (raws, warnings) = read(Path::new(PATH), text.as_bytes()).unwrap();
    assert_eq!(
        raws,
        vec![
            raw("ui.theme", json!("dark"), 3, 20),
            raw("ui.font_size", json!(16), 4, 18),
            raw("agents.a", json!({"command": "/x"}), 5, 20),
        ]
    );
    assert_eq!(warnings, Vec::new());
}

#[test]
fn a_byte_order_mark_is_skipped_and_columns_start_after_it() {
    let (raws, _) = read(
        Path::new(PATH),
        b"\xEF\xBB\xBF{\"ui\": {\"theme\": \"light\"}}",
    )
    .unwrap();
    assert_eq!(raws, vec![raw("ui.theme", json!("light"), 1, 18)]);
}

#[test]
fn an_unknown_section_and_a_section_that_is_no_object_are_warnings() {
    let text = "{\"telemetry\": {\"on\": true},\n\"ui\": 3, \"matters\": {}}";
    let (raws, warnings) = read(Path::new(PATH), text.as_bytes()).unwrap();
    assert_eq!(raws, Vec::new());
    assert_eq!(
        warnings,
        vec![
            Warning {
                origin: at(1, 15),
                kind: WarningKind::UnknownKey {
                    key: "telemetry".to_owned()
                },
            },
            Warning {
                origin: at(2, 7),
                kind: WarningKind::InvalidValue {
                    key: "ui".to_owned(),
                    message: "a section must be an object".to_owned(),
                },
            },
        ]
    );
}

#[test]
fn a_syntax_error_has_its_place_and_a_message_without_the_place() {
    let error = read(Path::new(PATH), b"{\n  \"ui\": {\"theme\": }\n}").unwrap_err();
    assert_eq!(
        error,
        Syntax {
            position: Position {
                line: 2,
                column: 19
            },
            message: "expected value".to_owned(),
        }
    );
}

#[test]
fn the_file_must_be_one_object() {
    let error = read(Path::new(PATH), b"[1, 2]").unwrap_err();
    // The parser's place for a problem at the very start.
    assert_eq!(error.position, Position { line: 1, column: 0 });
    assert_eq!(
        error.message,
        "invalid type: sequence, expected a JSON object"
    );
}

#[test]
fn trailing_commas_inside_sections_and_values_are_read() {
    let text = "{\"ui\": {\"theme\": \"dark\",},\n\"agents\": {\"a\": {\"args\": [\"x\",], \
                \"command\": \"/x\",}, },}";
    let (raws, warnings) = read(Path::new(PATH), text.as_bytes()).unwrap();
    assert_eq!(
        raws,
        vec![
            raw("ui.theme", json!("dark"), 1, 18),
            raw("agents.a", json!({"args": ["x"], "command": "/x"}), 2, 17),
        ]
    );
    assert_eq!(warnings, []);
}

#[test]
fn only_trailing_commas_are_blanked() {
    let blanked = |text: &str| {
        String::from_utf8(blank_trailing_commas(text.as_bytes()).into_owned()).unwrap()
    };
    assert_eq!(blanked("[1, 2,]"), "[1, 2 ]");
    assert_eq!(blanked("{\"a\": 1 , \n }"), "{\"a\": 1   \n }");
    assert_eq!(
        blanked("[1, // c ,]\n /* , ] */ ]"),
        "[1  // c ,]\n /* , ] */ ]"
    );
    assert_eq!(blanked("[1, /* a ** b */ ]"), "[1  /* a ** b */ ]");
    // A string that ends in an escaped backslash ends at the next quote.
    assert_eq!(blanked(r#"["a\\",]"#), r#"["a\\" ]"#);
    // Commas in strings and comments, and commas before a value, stay.
    for text in [
        "[\",]\", 1]",
        "[\"\\\",]\"]",
        "[1 /* ,] */, 2]",
        "[1 // ,]\n, 2]",
        "[1,\n2]",
        "[1, /",
        "\"open, ]",
        "/* open",
        "[1, // open",
        "[1, / ]",
        "[1, */]",
    ] {
        assert_eq!(blanked(text), text, "{text:?}");
    }
    assert!(matches!(blank_trailing_commas(b"[1]"), Cow::Borrowed(_)));
}
