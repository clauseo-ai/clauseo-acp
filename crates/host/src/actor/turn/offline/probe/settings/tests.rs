//! The agent's own settings files (`settings.rs`): which move its route or host, and what a
//! launch's target becomes then. Each test works in a folder of its own under the temp folder.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;
use serde_json::json;

use super::{MAX_FILE, Moved, claude_moved, claude_shell_carries, codex_moved, meta_moved, routes};
use crate::actor::turn::offline::probe::{ReachClass, Target};
use crate::write;

/// A fresh folder for `test`, with `managed` and `config` folders in it.
fn folder(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "clauseo-probe-settings-{}-{test}",
        std::process::id()
    ));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root.join("managed")).unwrap();
    write::create_dir_all(&root.join("config")).unwrap();
    root
}

fn put(path: &Path, text: &str) {
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(path, text.as_bytes()).unwrap();
}

/// What Claude Code at `root` moves.
fn claude(root: &Path) -> Option<Moved> {
    claude_moved(&root.join("managed"), Some(&root.join("config")))
}

/// A plain Claude launch's target (`api.anthropic.com`, direct) once `moved` is read.
fn claude_target(moved: Option<Moved>) -> Target {
    let env = [(OsString::from("PATH"), OsString::from("bin"))].into();
    Target::at_launch(AgentKind::Claude, &env, false).unless_moved(moved)
}

/// The re-check's case: the user settings in the agent's Claude home set `ANTHROPIC_BASE_URL`
/// (or a proxy, any case) in `env`: the launch is not probed.
#[test]
fn a_user_settings_env_with_a_base_url_or_a_proxy_leaves_the_service_unknown() {
    for (test, key) in [("base-url", "ANTHROPIC_BASE_URL"), ("proxy", "https_proxy")] {
        let root = folder(test);
        let settings = json!({"syncClaudeAiSkills": false, "env": {key: "http://firm.example"}});
        put(&root.join("config/settings.json"), &settings.to_string());
        assert_eq!(claude(&root), Some(Moved::SettingsEnv), "{key}");
        assert_eq!(
            claude_target(claude(&root)),
            Target::Unknown(ReachClass::UnknownService)
        );
        write::remove_dir_all(&root).unwrap();
    }
}

/// The firm's `managed-settings.json` (and a drop-in), the global config and the cached remote
/// settings move it as the user settings do.
#[test]
fn managed_global_and_remote_settings_are_read_too() {
    let cases = [
        (
            "managed",
            "managed/managed-settings.json",
            "CLAUDE_CODE_USE_BEDROCK",
        ),
        (
            "drop-in",
            "managed/managed-settings.d/10-firm.json",
            "GOOGLE_CLOUD_PROJECT",
        ),
        ("global", "config/.claude.json", "AWS_REGION"),
        (
            "legacy-global",
            "config/.config.json",
            "VERTEX_REGION_CLAUDE_4",
        ),
        ("remote", "config/remote-settings.json", "HTTP_PROXY"),
    ];
    for (test, file, key) in cases {
        let root = folder(test);
        put(&root.join(file), &json!({"env": {key: "1"}}).to_string());
        assert_eq!(claude(&root), Some(Moved::SettingsEnv), "{file}");
        write::remove_dir_all(&root).unwrap();
    }
}

/// Unrelated keys, Clauseo's own home settings, a hidden drop-in and absent files change
/// nothing: the target stays the service.
#[test]
fn settings_with_only_unrelated_keys_leave_the_target_as_it_was() {
    let root = folder("unrelated");
    assert_eq!(claude(&root), None, "nothing there");
    let env = json!({"env": {"DISABLE_TELEMETRY": "1", "BASH_DEFAULT_TIMEOUT_MS": "1000"}});
    put(&root.join("config/settings.json"), &env.to_string());
    put(&root.join("config/.claude.json"), r#"{"theme": "dark"}"#);
    put(
        &root.join("managed/managed-settings.json"),
        r#"{"env": null}"#,
    );
    let hidden = json!({"env": {"ANTHROPIC_BASE_URL": "x"}});
    put(
        &root.join("managed/managed-settings.d/.old.json"),
        &hidden.to_string(),
    );
    put(
        &root.join("managed/managed-settings.d/notes.txt"),
        &hidden.to_string(),
    );
    assert_eq!(claude(&root), None);
    assert_eq!(claude_target(claude(&root)), claude_target(None));
    assert!(matches!(claude_target(None), Target::Service(_)));
    write::remove_dir_all(&root).unwrap();
}

/// A file that is there but does not parse, holds more than 1 MiB or is not a file (here a
/// folder) moves it: unsure means no probe.
#[test]
fn a_settings_file_that_cannot_be_read_leaves_the_service_unknown() {
    let root = folder("unreadable");
    put(&root.join("config/settings.json"), "{ not json");
    assert_eq!(claude(&root), Some(Moved::SettingsEnv), "garbage");
    let root = folder("too-big");
    let padding = " ".repeat(usize::try_from(MAX_FILE).unwrap());
    put(
        &root.join("config/settings.json"),
        &format!("{{}}{padding}"),
    );
    assert_eq!(claude(&root), Some(Moved::SettingsEnv), "over 1 MiB");
    let root = folder("not-a-file");
    write::create_dir_all(&root.join("managed/managed-settings.json")).unwrap();
    assert_eq!(claude(&root), Some(Moved::SettingsEnv), "a folder");
    let root = folder("env-not-object");
    put(
        &root.join("config/settings.json"),
        r#"{"env": "HTTPS_PROXY=x"}"#,
    );
    assert_eq!(claude(&root), Some(Moved::SettingsEnv), "env not an object");
    for test in ["unreadable", "too-big", "not-a-file", "env-not-object"] {
        write::remove_dir_all(&folder(test)).unwrap();
    }
}

/// The variables that move Claude Code, matched without case; others do not.
#[test]
fn route_variables_are_proxies_base_urls_and_providers() {
    for key in [
        "HTTPS_PROXY",
        "no_proxy",
        "ALL_PROXY",
        "ANTHROPIC_BASE_URL",
        "anthropic_bedrock_base_url",
        "CLAUDE_CODE_USE_VERTEX",
        "AWS_PROFILE",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "VERTEX_REGION_CLAUDE_3_5_SONNET",
    ] {
        assert!(routes(key), "{key}");
    }
    for key in [
        "PATH",
        "DISABLE_TELEMETRY",
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        "PROXY",
    ] {
        assert!(!routes(key), "{key}");
    }
}

/// Clauseo's own `_meta` (the hook's `--settings` object or the SDK's `env`) moves it the same
/// way; Clauseo's usual keys do not.
#[test]
fn clauseos_own_session_settings_are_read_the_same_way() {
    let meta = |options: serde_json::Value| {
        json!({"claudeCode": {"options": options}})
            .as_object()
            .unwrap()
            .clone()
    };
    let usual = meta(json!({"env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "false"},
        "settings": {"disableSkillShellExecution": true}}));
    assert!(!meta_moved(&usual));
    assert!(!meta_moved(&serde_json::Map::new()));
    assert!(meta_moved(&meta(
        json!({"settings": {"env": {"CLAUDE_CODE_USE_BEDROCK": "1"}}})
    )));
    assert!(meta_moved(&meta(
        json!({"env": {"HTTPS_PROXY": "http://p:1"}})
    )));
    assert!(meta_moved(&meta(json!({"settings": "firm-settings.json"}))));
}

/// The `config.toml` Clauseo writes in its own Codex home, as it writes it.
fn clauseo_codex_config() -> String {
    crate::agent::codex::home_config()
}

/// A reused Codex config with another provider, a provider with a base URL, a base URL or a
/// profile setting one moves it; Clauseo's own config, or `openai` named, does not.
#[test]
fn a_codex_config_with_a_provider_or_a_base_url_leaves_the_service_unknown() {
    let root = folder("codex");
    let file = root.join("config/config.toml");
    let moved = |text: &str| {
        put(&file, text);
        codex_moved(std::slice::from_ref(&file))
    };
    assert_eq!(codex_moved(std::slice::from_ref(&file)), None, "no file");
    assert_eq!(moved(&clauseo_codex_config()), None, "Clauseo's own");
    assert_eq!(
        moved("model = \"gpt-5\"\nmodel_provider = \"openai\"\n"),
        None
    );
    for text in [
        "model_provider = \"azure\"\n",
        "[model_providers.firm]\nname = \"Firm\"\nbase_url = \"https://llm.firm.example/v1\"\n",
        "openai_base_url = \"https://gateway.firm.example/v1\"\n",
        "chatgpt_base_url = \"https://chat.firm.example\"\n",
        "[profiles.work]\nmodel_provider = \"oss\"\n",
        "[profiles.work]\nchatgpt_base_url = \"https://chat.firm.example\"\n",
        "model_provider = [\"not\", \"a\", \"name\"]\n",
        "this is = not toml [",
    ] {
        assert_eq!(moved(text), Some(Moved::CodexConfig), "{text}");
    }
    // The system file, read before the home's, moves it too.
    let system = root.join("managed/config.toml");
    put(&system, "model_provider = \"amazon-bedrock\"\n");
    put(&file, &clauseo_codex_config());
    assert_eq!(codex_moved(&[system, file]), Some(Moved::CodexConfig));
    write::remove_dir_all(&root).unwrap();
}

/// A Codex target after its config is read: unknown when it moved, as it was otherwise.
#[test]
fn a_moved_codex_launch_is_not_probed() {
    let env = [(OsString::from("PATH"), OsString::from("bin"))].into();
    let target = Target::at_launch(AgentKind::Codex, &env, false);
    assert!(matches!(target, Target::Service(_)));
    assert_eq!(target.clone().unless_moved(None), target);
    assert_eq!(
        target.unless_moved(Some(Moved::CodexConfig)),
        Target::Unknown(ReachClass::UnknownService)
    );
}

/// Claude Code's policy settings turn `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR` off with any
/// value its `De()` does not read as true (`1`, `true`, `yes`, `on`, in any case, trimmed);
/// a policy file that cannot be parsed may too. Unset, or true, it stays on.
#[test]
fn a_policy_false_for_the_shell_folder_variable_keeps_the_folder_across_calls() {
    let root = folder("shell_carries");
    let managed = root.join("managed");
    assert!(!claude_shell_carries(&managed));
    let file = managed.join("managed-settings.json");
    for (value, carries) in [
        (json!("1"), false),
        (json!(" TRUE "), false),
        (json!("yes"), false),
        (json!("On"), false),
        (json!(true), false),
        (json!(1), false),
        (json!("0"), true),
        (json!("false"), true),
        (json!(""), true),
        (json!(false), true),
        (json!(null), true),
    ] {
        let env = json!({"env": {"CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR": value}});
        put(&file, &env.to_string());
        assert_eq!(claude_shell_carries(&managed), carries, "{value}");
    }
    put(&file, r#"{"env": {"OTHER": "0"}}"#);
    assert!(!claude_shell_carries(&managed));
    write::remove_file(&file).unwrap();
    let drop_in = managed.join("managed-settings.d").join("50-shell.json");
    put(
        &drop_in,
        r#"{"env": {"CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR": "no"}}"#,
    );
    assert!(claude_shell_carries(&managed));
    put(&drop_in, "{ not json");
    assert!(claude_shell_carries(&managed));
    write::remove_dir_all(&root).unwrap();
}
