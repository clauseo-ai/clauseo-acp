//! The profile alone: the instructions, Claude's `_meta` fragment, Codex's keys, the `browser`
//! entry, the launch variables and the placeholder contract. No file is read or written.

use std::path::{Path, PathBuf};

use clauseo_profile::instructions::{TOOLS, word_count};
use clauseo_profile::placeholders::{
    self, BROWSER_EXECUTABLE, IN_INSTRUCTIONS, IN_SERVERS, MATTER_NOTES, PROXY_PORT, SESSION_DIR,
    SESSION_ID, WORK_FOLDER,
};
use clauseo_profile::{BrowserSidecars, ClauseoProfile, CodexConfig, ProfilePaths};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

fn paths() -> ProfilePaths {
    ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        PathBuf::from("/app/stetdoc/stetdoc"),
        BrowserSidecars {
            clauseo_mcp: PathBuf::from("/app/clauseo-mcp"),
            agent_browser: PathBuf::from("/app/agent-browser"),
        },
    )
}

fn profile() -> ClauseoProfile {
    ClauseoProfile::new(paths(), None)
}

/// Pretty JSON with Windows' `\` separators shown as `/`, so one snapshot serves every OS.
#[cfg(test)]
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap()
        .replace("\\\\", "/")
}

#[test]
fn the_instructions_are_the_tools_part_under_300_words() {
    let text = profile().instructions();
    assert!(word_count(TOOLS) < 300, "{} words", word_count(TOOLS));
    assert!(text.contains("--author \"Clauseo\""));
    assert!(text.contains("{work_folder}"));
    assert!(!text.contains("{author}"));
    insta::assert_snapshot!(text);
}

#[test]
fn no_name_is_clauseo_and_a_name_loses_what_a_shell_or_the_host_would_act_on() {
    assert_eq!(profile().author(), "Clauseo");
    let named = ClauseoProfile::new(
        paths(),
        Some("  A. \"Mehta\" $(rm) `x` {session_id}\n".into()),
    );
    assert_eq!(named.author(), "A. Mehta (rm) x session_id");
    assert!(
        named
            .instructions()
            .contains("--author \"A. Mehta (rm) x session_id\"")
    );
    assert_eq!(
        named.env().0["STETDOC_AUTHOR"],
        "A. Mehta (rm) x session_id"
    );
    assert_eq!(
        ClauseoProfile::new(paths(), Some("\"$`".into())).author(),
        "Clauseo"
    );
}

#[test]
fn the_claude_fragment_holds_only_the_profiles_keys() {
    let meta = profile().session_meta(Some(AgentKind::Claude)).unwrap();
    let options = meta["claudeCode"]["options"].as_object().unwrap();
    let keys: Vec<&str> = options.keys().map(String::as_str).collect();
    assert_eq!(keys, ["allowedTools", "plugins", "settings"]);
    for forced in [
        "strictMcpConfig",
        "settingSources",
        "mcpServers",
        "env",
        "extraArgs",
    ] {
        assert!(!options.contains_key(forced), "{forced}");
    }
    let settings = options["settings"].as_object().unwrap();
    assert!(!settings.contains_key("hooks") && !settings.contains_key("disableAllHooks"));
    insta::assert_snapshot!(pretty(&Value::Object(meta)));
}

#[test]
fn the_allow_list_is_the_browser_and_stetdocs_read_only_commands() {
    assert_eq!(
        profile().claude_options().allowed_tools,
        [
            "mcp__browser",
            "Bash(stetdoc view:*)",
            "Bash(stetdoc inspect:*)",
            "Bash(stetdoc check:*)",
            "Bash(stetdoc lookup:*)",
            "Bash(stetdoc capabilities:*)",
        ]
    );
    assert_eq!(
        profile().claude_options().disallowed_tools,
        Vec::<String>::new()
    );
}

#[test]
fn every_claude_session_turns_skill_shell_execution_off() {
    let meta = profile().session_meta(Some(AgentKind::Claude)).unwrap();
    assert_eq!(
        meta["claudeCode"]["options"]["settings"],
        json!({"disableSkillShellExecution": true})
    );
}

#[test]
fn the_codex_fragment_is_the_instructions_alone() {
    let keys = profile().codex_keys(Some(AgentKind::Codex));
    insta::assert_snapshot!(pretty(&Value::Object(keys)));
}

#[test]
fn reserved_codex_keys_never_leave_the_profile() {
    let mut keys = Map::new();
    for key in CodexConfig::RESERVED {
        keys.insert(key.to_owned(), json!({}));
    }
    keys.insert("developer_instructions".to_owned(), json!("x"));
    let config = CodexConfig::new(keys);
    assert_eq!(
        config.keys().keys().collect::<Vec<_>>(),
        ["developer_instructions"]
    );
}

#[test]
fn kind_specific_parts_go_only_to_their_kind() {
    let profile = profile();
    assert!(profile.session_meta(Some(AgentKind::Codex)).is_none());
    assert!(profile.session_meta(None).is_none());
    assert!(profile.codex_keys(Some(AgentKind::Claude)).is_empty());
    assert!(profile.codex_keys(None).is_empty());
    assert!(profile.skill_files(None).is_none());
    for kind in [None, Some(AgentKind::Claude), Some(AgentKind::Codex)] {
        let names: Vec<String> = profile.servers(kind).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["browser"]);
    }
}

#[test]
fn the_browser_entry_runs_the_guard_with_four_variables() {
    let [browser] = profile()
        .servers(Some(AgentKind::Claude))
        .try_into()
        .unwrap();
    assert_eq!(browser.command, PathBuf::from("/app/clauseo-mcp"));
    assert_eq!(
        browser.args,
        [
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "{session_dir}/browser",
            "--",
            "mcp",
            "--tools",
            "core",
        ]
    );
    let env: Vec<(&str, String)> = browser
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.replace('\\', "/")))
        .collect();
    assert_eq!(
        env,
        [
            (
                "AGENT_BROWSER_CONFIG",
                "/data/browser/agent-browser.json".to_owned()
            ),
            (
                "AGENT_BROWSER_EXECUTABLE_PATH",
                "{browser_executable}".to_owned()
            ),
            (
                "AGENT_BROWSER_PROXY",
                "http://127.0.0.1:{proxy_port}".to_owned()
            ),
            ("AGENT_BROWSER_SESSION", "{session_id}".to_owned()),
        ]
    );
}

#[test]
fn every_token_the_profile_sends_is_one_the_host_is_told_to_fill() {
    let profile = profile();
    let mut texts = vec![profile.instructions()];
    let tokens: Vec<&str> = IN_INSTRUCTIONS.iter().chain(&IN_SERVERS).copied().collect();
    for server in profile.servers(None) {
        texts.extend(server.args);
        texts.extend(server.env.into_values());
    }
    for text in texts {
        let mut rest = text.as_str();
        while let Some(at) = rest.find('{') {
            let from = rest.get(at..).unwrap();
            assert!(
                tokens.iter().any(|t| from.starts_with(t)),
                "unlisted token in {text}"
            );
            rest = from.get(1..).unwrap();
        }
    }
}

#[test]
fn a_folder_named_like_a_token_stays_literal_and_json_escapes_it() {
    // The host fills decoded strings, then its encoder escapes them: here JSON.
    let folder = r#"/Users/a "b"/{session_id}"#;
    let filled = placeholders::fill(
        "--cwd {session_dir}/browser",
        &IN_SERVERS,
        &[
            (SESSION_DIR, folder),
            (SESSION_ID, "s1"),
            (PROXY_PORT, "1"),
            (BROWSER_EXECUTABLE, "/c"),
        ],
    )
    .unwrap();
    assert_eq!(filled, r#"--cwd /Users/a "b"/{session_id}/browser"#);
    let encoded = Value::String(filled).to_string();
    assert_eq!(encoded, r#""--cwd /Users/a \"b\"/{session_id}/browser""#);
    let instructions = placeholders::fill(
        &profile().instructions(),
        &IN_INSTRUCTIONS,
        &[
            (WORK_FOLDER, "/run/{matter_notes}/work"),
            (MATTER_NOTES, "/notes"),
        ],
    )
    .unwrap();
    assert!(instructions.contains("/run/{matter_notes}/work"));
}

#[test]
fn an_unfillable_instruction_token_is_an_error_the_host_fails_the_launch_on() {
    let error = placeholders::fill(
        &profile().instructions(),
        &IN_INSTRUCTIONS,
        &[(MATTER_NOTES, "/notes")],
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "no value for the placeholder {work_folder}"
    );
}

#[test]
fn stetdocs_own_folder_goes_first_on_path() {
    assert_eq!(profile().path_prepend(), Path::new("/app/stetdoc"));
}
