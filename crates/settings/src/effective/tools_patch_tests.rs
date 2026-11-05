//! `Effective::apply_tools_patch`: the lock check, the write, and memory left alone when either
//! fails. The fakes and policy sources are the ones in `crate::testing`.

use super::Effective;
use crate::error::Error;
use crate::schema::ToolsSettings;
use crate::testing::{MemFiles, locations, policy, user};
use crate::tools::ToolsPatch;
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    locations().settings_file().to_owned()
}

/// The settings file's text, or `None` when there is no file.
fn file_text(files: &MemFiles) -> Option<String> {
    files.text(&settings_path())
}

/// A patch that turns web search off and the browser on.
fn search_off_browser_on() -> ToolsPatch {
    ToolsPatch {
        web_search: Some(false),
        browser: Some(true),
    }
}

#[test]
fn apply_tools_patch_writes_then_sets_memory_and_a_fresh_load_agrees() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .apply_tools_patch(&files, &search_off_browser_on())
        .unwrap();
    assert_eq!(
        effective.settings().tools,
        ToolsSettings {
            web_search: false,
            browser: true,
            connectors: true,
        }
    );
    assert_eq!(
        file_text(&files).unwrap(),
        "{\n  \"tools\": {\n    \"web_search\": false,\n    \"browser\": true\n  }\n}\n"
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().tools, effective.settings().tools);
    assert_eq!(fresh.warnings(), []);
}

#[test]
fn a_partial_patch_changes_only_the_named_field() {
    let files =
        user(r#"{"tools": {"web_search": true, "browser": true}, "ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .apply_tools_patch(
            &files,
            &ToolsPatch {
                web_search: Some(false),
                ..ToolsPatch::default()
            },
        )
        .unwrap();
    assert!(!effective.settings().tools.web_search);
    assert!(
        effective.settings().tools.browser,
        "browser stays on in memory"
    );
    let saved: serde_json::Value = serde_json::from_str(&file_text(&files).unwrap()).unwrap();
    assert_eq!(saved["tools"]["web_search"], false);
    assert_eq!(
        saved["tools"]["browser"], true,
        "browser stays on in the file"
    );
    assert_eq!(saved["ui"]["theme"], "dark", "the rest of the file stays");
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().tools, effective.settings().tools);
}

#[test]
fn a_patch_naming_a_locked_key_is_refused_whole_and_writes_nothing() {
    let files = user(r#"{"tools": {"web_search": true, "browser": true}}"#);
    let firm = policy("firm", r#"{"tools": {"browser": false}}"#);
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let before = effective.settings().clone();
    let original = file_text(&files);
    // The web search key is unlocked and comes first in the patch, but it must not be written.
    let error = effective
        .apply_tools_patch(&files, &search_off_browser_on())
        .unwrap_err();
    assert!(
        matches!(&error, Error::Locked { key } if key == "tools.browser"),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "tools.browser is set by policy and cannot be changed"
    );
    assert_eq!(file_text(&files), original);
    assert_eq!(effective.settings(), &before);
}

#[test]
fn each_locked_tool_key_is_reported_by_its_own_name() {
    for (firm_text, patch, key) in [
        (
            r#"{"tools": {"web_search": true}}"#,
            ToolsPatch {
                web_search: Some(false),
                ..ToolsPatch::default()
            },
            "tools.web_search",
        ),
        (
            r#"{"tools": {"browser": false}}"#,
            ToolsPatch {
                browser: Some(true),
                ..ToolsPatch::default()
            },
            "tools.browser",
        ),
        // A policy value that does not type still locks its key (fail closed).
        (
            r#"{"tools": {"browser": "yes"}}"#,
            ToolsPatch {
                browser: Some(true),
                ..ToolsPatch::default()
            },
            "tools.browser",
        ),
        // A policy source that cannot be read locks both off, and the patch of either is refused.
        (
            "{ not json",
            ToolsPatch {
                web_search: Some(true),
                ..ToolsPatch::default()
            },
            "tools.web_search",
        ),
    ] {
        let files = MemFiles::default();
        let firm = policy("firm", firm_text);
        let mut effective = Effective::load(&locations(), &files, &[&firm]);
        let error = effective.apply_tools_patch(&files, &patch).unwrap_err();
        assert!(
            matches!(&error, Error::Locked { key: named } if named == key),
            "{firm_text}: {error}"
        );
        assert_eq!(file_text(&files), None, "{firm_text}");
    }
}

#[test]
fn a_patch_equal_to_the_locked_value_is_refused_too() {
    let files = MemFiles::default();
    let firm = policy("firm", r#"{"tools": {"browser": true}}"#);
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let same = ToolsPatch {
        browser: Some(true),
        ..ToolsPatch::default()
    };
    let error = effective.apply_tools_patch(&files, &same).unwrap_err();
    assert!(
        matches!(&error, Error::Locked { key } if key == "tools.browser"),
        "{error}"
    );
    assert_eq!(file_text(&files), None);
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
            .apply_tools_patch(&files, &search_off_browser_on())
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
    let text = "{\"tools\": ";
    let files = user(text);
    let mut effective = Effective::load(&locations(), &files, &[]);
    let before = effective.settings().clone();
    let error = effective
        .apply_tools_patch(&files, &search_off_browser_on())
        .unwrap_err();
    assert!(
        matches!(
            &error,
            Error::Parse { path, line: 1, column: 10, .. } if *path == settings_path()
        ),
        "{error}"
    );
    assert_eq!(file_text(&files).as_deref(), Some(text));
    assert_eq!(effective.settings(), &before);
}

#[test]
fn an_empty_patch_leaves_a_commented_file_byte_identical() {
    let text = "// my settings\n{\n  // the switches\n  \"tools\": {\"browser\": false}\n}\n";
    let files = user(text);
    let mut effective = Effective::load(&locations(), &files, &[]);
    let before = effective.settings().clone();
    effective
        .apply_tools_patch(&files, &ToolsPatch::default())
        .unwrap();
    assert_eq!(file_text(&files).as_deref(), Some(text));
    assert_eq!(effective.settings(), &before);
    let no_file = MemFiles::default();
    effective
        .apply_tools_patch(&no_file, &ToolsPatch::default())
        .unwrap();
    assert_eq!(file_text(&no_file), None);
}

#[test]
fn a_patch_refuses_unknown_fields() {
    for text in [
        r#"{"density": "compact"}"#,
        r#"{"web_search": true, "agents": {}}"#,
        r#"{"tools": {}}"#,
    ] {
        assert!(
            serde_json::from_str::<ToolsPatch>(text).is_err(),
            "{text} decoded"
        );
    }
    let patch: ToolsPatch = serde_json::from_str(r#"{"browser": false}"#).unwrap();
    assert_eq!(
        patch,
        ToolsPatch {
            web_search: None,
            browser: Some(false),
        }
    );
}
