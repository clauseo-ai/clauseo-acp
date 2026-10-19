//! Tests for the `capabilities` check: `scan` and `inline_violations` on strings. The `run`
//! cases on scratch trees are the planted cases in `tests/planted.rs`, built below.

mod cases;
pub(super) mod planted;

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use crate::workspace::Context;

const PATH: &str = "crates/desktop/capabilities/main.json";

const CLEAN: &str = r#"{
  "identifier": "main",
  "description": "Grants nothing: refs: and prose are fine.",
  "windows": ["main"],
  "permissions": []
}"#;

fn violation(line: u32, message: &str, fix: &str) -> Violation {
    Violation {
        path: PATH.to_owned(),
        line,
        message: message.to_owned(),
        fix: fix.to_owned(),
    }
}

#[test]
fn a_capability_with_an_empty_list_passes() {
    assert_eq!(scan(PATH, CLEAN), Vec::new());
}

#[test]
fn core_default_fails_at_its_line() {
    let text = "{\n  \"permissions\": [\n    \"core:default\"\n  ]\n}";
    assert_eq!(
        scan(PATH, text),
        vec![violation(3, "capability grants `core:default`", FIX_CORE)]
    );
}

#[test]
fn remote_fails_at_its_line() {
    let text = "{\n  \"remote\": { \"urls\": [\"https://*\"] }\n}";
    assert_eq!(
        scan(PATH, text),
        vec![violation(
            2,
            "capability file mentions `remote`",
            FIX_REMOTE
        )]
    );
}

#[test]
fn remote_in_prose_fails_with_the_reword_hint() {
    let text = "{\n  \"description\": \"never loads a remote page\"\n}";
    assert_eq!(
        scan(PATH, text),
        vec![violation(
            2,
            "capability file mentions `remote`",
            FIX_REMOTE
        )]
    );
    assert!(FIX_REMOTE.contains("or reword the text"));
}

#[test]
fn a_quoted_fs_permission_fails_and_names_the_whole_string() {
    let text = "{\n  \"permissions\": [\"fs:allow-read\"]\n}";
    assert_eq!(
        scan(PATH, text),
        vec![violation(
            2,
            "capability grants `fs:allow-read`",
            FIX_PREFIX
        )]
    );
}

#[test]
fn an_identifier_object_with_a_prefix_fails_and_toml_single_quotes_count() {
    let object = "[{ \"identifier\": \"fs:scope\" }]";
    assert_eq!(
        scan(PATH, object)[0].message,
        "capability grants `fs:scope`"
    );
    let toml_path = "crates/desktop/capabilities/main.toml";
    let single = "identifier = \"main\"\npermissions = ['shell:allow-open']\n";
    assert_eq!(
        scan(toml_path, single),
        vec![Violation {
            path: toml_path.to_owned(),
            ..violation(2, "capability grants `shell:allow-open`", FIX_PREFIX)
        }]
    );
}

#[test]
fn escaped_json_strings_are_decoded_and_reported_at_their_key_or_line_one() {
    let core = "{\n  \"permissions\": [\n    \"core\\u003adefault\"\n  ]\n}";
    assert_eq!(
        scan(PATH, core),
        vec![violation(
            2,
            "capability grants `core:default` (written with escapes)",
            FIX_CORE
        )]
    );
    let fs = "{\n  \"permissions\": [\"\\u0066s:allow-read\"]\n}";
    assert_eq!(
        scan(PATH, fs),
        vec![violation(
            2,
            "capability grants `fs:allow-read` (written with escapes)",
            FIX_PREFIX
        )]
    );
    let remote = "{\n  \"identifier\": \"main\",\n  \"re\\u006dote\": { \"urls\": [] }\n}";
    assert_eq!(
        scan(PATH, remote),
        vec![violation(
            1,
            "capability file mentions `remote` (written with escapes)",
            FIX_REMOTE
        )]
    );
}

#[test]
fn an_escaped_toml_string_is_decoded() {
    let toml_path = "crates/desktop/capabilities/main.toml";
    let text =
        "identifier = \"main\"\npermissions = [\"\\u0066s:allow-read\", \"core\\u003Adefault\"]\n";
    let found = scan(toml_path, text);
    let lines: Vec<(u32, &str)> = found.iter().map(|v| (v.line, v.message.as_str())).collect();
    assert_eq!(
        lines,
        [
            (
                2,
                "capability grants `fs:allow-read` (written with escapes)"
            ),
            (2, "capability grants `core:default` (written with escapes)"),
        ]
    );
}

#[test]
fn a_string_written_twice_points_at_both_lines() {
    let text = "[\n  { \"permissions\": [\"core:default\"] },\n  { \"permissions\": [\"core:default\"] }\n]";
    let lines: Vec<u32> = scan(PATH, text).iter().map(|v| v.line).collect();
    assert_eq!(lines, [2, 3]);
}

#[test]
fn json5_that_is_not_plain_json_and_invalid_files_fail_at_line_one() {
    let json5 = "crates/desktop/capabilities/main.json5";
    assert_eq!(scan(json5, "{ \"permissions\": [] }"), Vec::new());
    assert_eq!(
        scan(json5, "{ permissions: [] }"),
        vec![Violation {
            path: json5.to_owned(),
            ..violation(
                1,
                "json5 capability files are not supported; use .json",
                FIX_JSON5
            )
        }]
    );
    assert_eq!(scan(PATH, "{ \"a\": ")[0].fix, FIX_SYNTAX);
    assert_eq!(scan("x/main.toml", "a = ")[0].fix, FIX_SYNTAX);
}

#[test]
fn a_line_with_core_default_and_a_prefix_fails_twice() {
    let text = "[\"core:default\", \"dialog:allow-ask\"]";
    let messages: Vec<String> = scan(PATH, text).into_iter().map(|v| v.message).collect();
    assert_eq!(
        messages,
        [
            "capability grants `core:default`",
            "capability grants `dialog:allow-ask`"
        ]
    );
}

#[test]
fn one_string_that_trips_two_rules_points_both_at_its_line_without_escapes() {
    let text = "{\n  \"identifier\": \"main\",\n  \"description\": \"remote pages never get core:default\"\n}";
    assert_eq!(
        scan(PATH, text),
        vec![
            violation(3, "capability file mentions `remote`", FIX_REMOTE),
            violation(3, "capability grants `core:default`", FIX_CORE),
        ]
    );
}

const CONF: &str = r#"{
  "app": {
    "security": {
      "capabilities": [
        "main",
        {
          "identifier": "inline",
          "windows": ["main"],
          "permissions": ["core:default", "fs:allow-read"]
        }
      ]
    }
  }
}"#;

#[test]
fn an_inline_capability_object_fails_at_its_lines_in_the_config() {
    let conf = "crates/desktop/tauri.conf.json";
    let line = 9;
    assert!(
        CONF.lines()
            .nth(8)
            .is_some_and(|text| text.contains("core:default"))
    );
    let at = |message: &str, fix: &str| Violation {
        path: conf.to_owned(),
        ..violation(line, message, fix)
    };
    assert_eq!(
        inline_violations(conf, CONF),
        vec![
            at("capability grants `core:default`", FIX_CORE),
            at("capability grants `fs:allow-read`", FIX_PREFIX),
        ]
    );
}

#[test]
fn an_escaped_inline_string_points_at_the_capabilities_key() {
    let conf = "crates/desktop/tauri.conf.json";
    let text = CONF.replace("\"core:default\", ", "\"core\\u003adefault\", ");
    assert_eq!(
        inline_violations(conf, &text),
        vec![
            Violation {
                path: conf.to_owned(),
                ..violation(
                    4,
                    "capability grants `core:default` (written with escapes)",
                    FIX_CORE
                )
            },
            Violation {
                path: conf.to_owned(),
                ..violation(9, "capability grants `fs:allow-read`", FIX_PREFIX)
            },
        ]
    );
}

#[test]
fn string_capabilities_and_an_invalid_config_pass_inline() {
    let conf = "crates/desktop/tauri.conf.json";
    let names = "{\"app\": {\"security\": {\"capabilities\": [\"main\", \"fs:x\"]}}}";
    assert_eq!(inline_violations(conf, names), Vec::new());
    assert_eq!(inline_violations(conf, "{ \"app\": "), Vec::new());
}
