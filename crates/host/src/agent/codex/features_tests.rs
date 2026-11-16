//! The two features every Codex launch turns off (`LAUNCH_FEATURES`): in the home's
//! `config.toml`, as one `[features]` table, and in `CODEX_CONFIG`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::tests::{home_in, object, scratch};
use super::{
    config_on, home_config, launch_env, make_home, without_login_shell, write_home_config,
};
use crate::write;

/// The `[features]` table of a home `config.toml` text, parsed as Codex parses it.
fn home_features(text: &str) -> toml::Table {
    let parsed: toml::Table = text.parse().unwrap();
    parsed["features"].as_table().unwrap().clone()
}

/// How many lines of `text` set the feature `name`, dotted (`features.name = …`) or inside a
/// `[features]` table (`name = …`).
fn feature_lines(text: &str, name: &str) -> usize {
    text.lines()
        .filter(|line| {
            let key = line.split('=').next().unwrap_or("").trim();
            key == name || key == format!("features.{name}")
        })
        .count()
}

/// The home's `config.toml` turns goals and terminal sessions off under `[features]`, beside the
/// limits crate's features, as one table.
#[test]
fn home_config_turns_off_goals_and_tty_sessions() {
    let text = home_config();
    let features = home_features(&text);
    assert_eq!(features["goals"].as_bool(), Some(false), "{text}");
    assert_eq!(
        features["unified_exec_tty"].as_bool(),
        Some(false),
        "{text}"
    );
    for name in ["apps", "plugins", "skill_mcp_dependency_install"] {
        assert_eq!(features[name].as_bool(), Some(false), "{name}: {text}");
    }
    assert!(
        !text.lines().any(|line| line.trim() == "[features]"),
        "{text}"
    );
}

/// `CODEX_CONFIG` carries both keys in its `features` object, on Windows and elsewhere, and
/// keeps them when the session's shell keys add `features.shell_snapshot`; a settings or
/// profile `features` object turns neither back on.
#[test]
fn codex_config_turns_off_goals_and_tty_sessions() {
    let settings = BTreeMap::from([(
        "CODEX_CONFIG".to_owned(),
        json!({"features": {"goals": true, "unified_exec_tty": true}}).to_string(),
    )]);
    let profile = object(json!({"features": {"goals": true, "unified_exec_tty": true}}));
    for windows in [false, true] {
        let mut built = config_on(&settings, profile.clone(), None, windows);
        without_login_shell(&mut built);
        let features = &built["features"];
        assert_eq!(features["goals"], json!(false), "{features}");
        assert_eq!(features["unified_exec_tty"], json!(false), "{features}");
        assert_eq!(features["shell_snapshot"], json!(false), "{features}");
        assert_eq!(features["apps"], json!(false), "{features}");
    }
    let env = launch_env(Path::new("/home"), &BTreeMap::new(), Map::new(), None);
    let text = env[&OsString::from("CODEX_CONFIG")].to_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["features"]["goals"], json!(false), "{text}");
    assert_eq!(
        parsed["features"]["unified_exec_tty"],
        json!(false),
        "{text}"
    );
}

/// A home `config.toml` that already had its own `[features]` table, with both keys on, is
/// written over: the file then sets each key exactly once, off, in one `features` table.
#[test]
fn home_config_over_a_features_table_sets_each_key_once() {
    let root = scratch("home-features");
    let home = home_in(&root);
    make_home(&root, &home).unwrap();
    let path = home.join("config.toml");
    write::write_file(
        &path,
        b"[features]\ngoals = true\nunified_exec_tty = true\napps = true\n",
    )
    .unwrap();
    write_home_config(&home).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for name in ["goals", "unified_exec_tty", "apps"] {
        assert_eq!(feature_lines(&text, name), 1, "{name}: {text}");
    }
    let features = home_features(&text);
    assert_eq!(features["goals"].as_bool(), Some(false), "{text}");
    assert_eq!(
        features["unified_exec_tty"].as_bool(),
        Some(false),
        "{text}"
    );
    assert_eq!(features["apps"].as_bool(), Some(false), "{text}");
    write::remove_dir_all(&root).unwrap();
}
