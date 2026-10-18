//! Tests for the `capabilities` check: `scan` on single files, and `run` on scratch trees.

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
fn the_check_is_registered_under_its_name() {
    assert!(
        crate::checks::ALL
            .iter()
            .any(|check| check.name == "capabilities")
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

#[test]
fn run_scans_inline_capabilities_in_the_config() {
    let dir = Scratch::new("capabilities-inline");
    let caps = dir.join("crates/desktop/capabilities");
    fs::create_dir_all(&caps).expect("create the capability dir");
    fs::write(caps.join("main.json"), CLEAN).expect("write main.json");
    fs::write(dir.join("crates/desktop/tauri.conf.json"), CONF).expect("write the config");
    let found = run(&desktop_context(&dir));
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .all(|v| v.path == "crates/desktop/tauri.conf.json" && v.line == 9)
    );
}

#[test]
fn a_ds_store_and_a_backup_file_are_ignored() {
    let dir = Scratch::new("capabilities-ignored");
    let caps = dir.join("crates/desktop/capabilities");
    fs::create_dir_all(&caps).expect("create the capability dir");
    fs::write(caps.join("main.json"), CLEAN).expect("write main.json");
    fs::write(
        caps.join(".DS_Store"),
        b"\x00\x00\x00\x01Bud1\xff\xfe\x00remote",
    )
    .expect("write .DS_Store");
    fs::write(caps.join("main.json~"), "[\"core:default\"]").expect("write main.json~");
    assert_eq!(run(&desktop_context(&dir)), Vec::new());
}

#[test]
fn a_missing_desktop_member_fails_at_the_root_manifest() {
    let dir = Scratch::new("capabilities-no-member");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    let found = run(&ctx);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.toml", 1));
}

#[test]
fn a_missing_capabilities_folder_fails() {
    let dir = Scratch::new("capabilities-missing");
    assert_eq!(
        run(&desktop_context(&dir)),
        vec![Violation {
            path: "crates/desktop/capabilities".to_owned(),
            line: 1,
            message: "the capabilities folder `crates/desktop/capabilities` is missing".to_owned(),
            fix: "restore it with main.json: Tauri reads the window's grants from it, and this check guards them".to_owned(),
        }]
    );
}

#[test]
fn a_moved_desktop_crate_is_found_by_its_package_name() {
    let dir = Scratch::new("capabilities-moved");
    let caps = dir.join("apps/shell/capabilities");
    fs::create_dir_all(&caps).expect("create the capability dir");
    fs::write(caps.join("main.json"), "[\"core:default\"]").expect("write main.json");
    let mut ctx = desktop_context(&dir);
    ctx.members[0].dir = "apps/shell".to_owned();
    let found = run(&ctx);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "apps/shell/capabilities/main.json");
}

#[test]
fn run_reads_nested_capability_files_and_reports_their_repo_path() {
    let dir = Scratch::new("capabilities-run");
    let nested = dir.join("crates/desktop/capabilities/extra");
    fs::create_dir_all(&nested).expect("create the capability dirs");
    fs::write(dir.join("crates/desktop/capabilities/main.json"), CLEAN).expect("write main.json");
    fs::write(
        nested.join("local.toml"),
        "permissions = [\"fs:allow-read\"]\n",
    )
    .expect("write local.toml");
    assert_eq!(
        run(&desktop_context(&dir)),
        vec![Violation {
            path: "crates/desktop/capabilities/extra/local.toml".to_owned(),
            ..violation(1, "capability grants `fs:allow-read`", FIX_PREFIX)
        }]
    );
}
