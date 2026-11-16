//! Codex's Code Mode stays on (stage C, codex findings §1 to §3). Codex 0.162.1 runs Code Mode
//! because the model catalog says so: every ChatGPT-plan model Clauseo uses carries
//! `"tool_mode": "code_mode_only"`, and `requested_tool_mode` (`core/src/tools/mod.rs:75-96`)
//! reads `features.code_mode*` only when the catalog sets no `tool_mode`. The one thing that turns
//! it off is a `model_catalog_json` the app-server starts with: from the home's `config.toml`
//! ([`home_config`]) or a `-c` on `codex app-server` (the limits reader's
//! `clauseo_limits::CODEX_OVERRIDES`, or a `CODEX_PATH` wrapper). These tests fail when anything
//! Clauseo gives Codex names the catalog, a tool mode or a Code Mode feature.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::{Value, json};

use super::{DROPPED_VARS, config_on, home_config, launch_env, without_login_shell};

/// The words that name what could turn Code Mode off: the model catalog, a tool mode, or a
/// Code Mode feature (`code_mode`, `code_mode_only`, `code_mode_host` and the rest).
const CODE_MODE_WORDS: [&str; 4] = ["model_catalog", "tool_mode", "code_mode", "codemode"];

/// The first of [`CODE_MODE_WORDS`] in `text`, in any case.
fn code_mode_word(text: &str) -> Option<&'static str> {
    let text = text.to_ascii_lowercase();
    CODE_MODE_WORDS.into_iter().find(|word| text.contains(word))
}

/// What a settings `CODEX_CONFIG` or a profile hook could try: each way of turning Code Mode
/// off, with a model the host keeps.
fn turning_it_off() -> Value {
    json!({
        "model": "gpt-6-luna",
        "model_catalog_json": "/tmp/catalog-direct.json",
        "features": {"code_mode": false, "code_mode_only": false},
        "features.code_mode_only": false,
        "tool_mode": "direct",
        "profiles": {"x": {"model_catalog_json": "/tmp/c.json"}},
    })
}

/// The app-server's start-up config never names the catalog, a tool mode or a Code Mode feature:
/// neither the home's `config.toml` nor the `-c` overrides the limits reader starts it with.
#[test]
fn codex_starts_with_nothing_that_turns_code_mode_off() {
    let home = home_config();
    for line in home.lines().filter(|line| !line.starts_with('#')) {
        assert_eq!(code_mode_word(line), None, "config.toml: {line}");
    }
    for set in clauseo_limits::CODEX_OVERRIDES {
        assert_eq!(code_mode_word(set), None, "app-server -c {set}");
    }
}

/// No source reaches `CODEX_CONFIG` with the catalog, a tool mode or a Code Mode feature: not a
/// settings `CODEX_CONFIG`, not the profile's keys, not the guard's hooks, not the shell keys,
/// on Windows or elsewhere. And `CODEX_PATH`, the one way to put a `-c` on codex-acp's
/// app-server, never reaches the agent.
#[test]
fn no_codex_launch_carries_a_code_mode_override() {
    let settings = BTreeMap::from([("CODEX_CONFIG".to_owned(), turning_it_off().to_string())]);
    let Value::Object(profile) = turning_it_off() else {
        panic!("an object");
    };
    let record = Path::new("/data/browser/guard/s1/guard-blocks.jsonl");
    let record = Some((record, "00000000000001030000000000000000000000000045c2a9"));
    let Some(Value::Object(hooks)) =
        crate::agent::loads::codex_hook_config(Path::new("/app/Clauseo"), record)
    else {
        panic!("the guard's hooks");
    };
    for windows in [false, true] {
        let mut config = config_on(&settings, profile.clone(), Some(hooks.clone()), windows);
        without_login_shell(&mut config);
        assert_eq!(config.get("model"), Some(&json!("gpt-6-luna")));
        let text = Value::Object(config).to_string();
        assert_eq!(code_mode_word(&text), None, "CODEX_CONFIG: {text}");
    }
    let env = launch_env(
        Path::new("/data/agents/codex-acp/home"),
        &settings,
        profile,
        Some(hooks),
    );
    for (name, value) in &env {
        let pair = format!("{}={}", name.to_string_lossy(), value.to_string_lossy());
        assert_eq!(code_mode_word(&pair), None, "{pair}");
    }
    assert!(!env.contains_key(&OsString::from("CODEX_PATH")));
    assert!(DROPPED_VARS.contains(&"CODEX_PATH"));
}
