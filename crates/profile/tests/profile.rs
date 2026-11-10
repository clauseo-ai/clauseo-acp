//! The profile alone: the instructions, Claude's `_meta` fragment, Codex's keys, the `browser`
//! entry, the launch variables and the placeholder contract. No file is read or written.

use std::path::{Path, PathBuf};

use clauseo_profile::claude::{OPTION_KEYS, SETTINGS_KEYS};
use clauseo_profile::instructions::{TOOLS, word_count};
use clauseo_profile::placeholders::{
    self, BROWSER, BROWSER_DIR, BROWSER_EXECUTABLE, CDP_PORT, DAEMON_PORT, IN_INSTRUCTIONS,
    IN_SERVERS, MATTER_NOTES, PROXY_PORT, SESSION_ID, SOCKET_DIR, STREAM_PORT, WORK_FOLDER,
};
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, CodexConfig, Instructions, ProfileError, ProfilePaths,
};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

/// `path` made absolute on this OS: as it is on Unix, on drive `C:` on Windows.
fn abs(path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{path}"))
    } else {
        PathBuf::from(path)
    }
}

fn paths_with(app: &str, stetdoc: &str, sidecars: &str) -> Result<ProfilePaths, ProfileError> {
    ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        abs(app),
        abs(stetdoc),
        BrowserSidecars {
            clauseo_mcp: abs(&format!("{sidecars}/clauseo-mcp")),
            agent_browser: abs(&format!("{sidecars}/agent-browser")),
        },
    )
}

#[cfg(test)]
fn paths() -> ProfilePaths {
    paths_with("/app/clauseo", "/app/stetdoc/stetdoc", "/app").unwrap()
}

/// The profile with R21's tools part alone, so these snapshots stay R21's; R22's legal part is
/// tested in `tests/legal.rs`.
fn profile() -> ClauseoProfile {
    ClauseoProfile::new(paths(), None).with_instructions(Instructions::tools())
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
    assert_eq!(named.author(), "A. Mehta rm x sessionid");
    assert!(
        named
            .instructions()
            .contains("--author \"A. Mehta rm x sessionid\"")
    );
    assert_eq!(named.env().0["STETDOC_AUTHOR"], "A. Mehta rm x sessionid");
    assert_eq!(
        ClauseoProfile::new(paths(), Some("\"$`".into())).author(),
        "Clauseo"
    );
}

#[test]
fn the_author_keeps_letters_marks_digits_and_four_marks_of_punctuation_only() {
    for (given, kept) in [
        ("O'Brien-Smith, Jr.", "O'Brien-Smith, Jr."),
        // PowerShell reads curly quotes as quotes; cmd expands %VAR%.
        (
            "\u{201C}A\u{201D} \u{201E}B %USERPROFILE%",
            "A B USERPROFILE",
        ),
        // Bidi controls and zero-width characters go.
        ("Ana\u{202E}lis\u{2066}\u{200B}", "Analis"),
        // Devanagari keeps its vowel signs and virama.
        (
            "\u{0930}\u{093E}\u{092E}\u{094D} 2",
            "\u{0930}\u{093E}\u{092E}\u{094D} 2",
        ),
        ("Zo\u{0065}\u{0308}", "Zo\u{0065}\u{0308}"),
        ("<>|&;()*?~#^=+/\\:", "Clauseo"),
    ] {
        assert_eq!(
            ClauseoProfile::new(paths(), Some(given.into())).author(),
            kept,
            "{given}"
        );
    }
}

#[test]
fn the_claude_fragment_holds_only_the_profiles_keys() {
    let meta = profile().session_meta(Some(AgentKind::Claude)).unwrap();
    let options = meta["claudeCode"]["options"].as_object().unwrap();
    assert!(
        options
            .keys()
            .all(|key| OPTION_KEYS.contains(&key.as_str()))
    );
    assert!(
        options["settings"]
            .as_object()
            .unwrap()
            .keys()
            .all(|key| SETTINGS_KEYS.contains(&key.as_str()))
    );
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
fn a_settings_key_outside_the_list_never_reaches_the_fragment() {
    let options = profile()
        .claude_options()
        .with_setting("hooks", json!({"PreToolUse": []}))
        .with_setting("disableAllHooks", json!(true));
    let meta = options.meta_fragment();
    assert_eq!(
        meta["claudeCode"]["options"]["settings"],
        json!({"disableSkillShellExecution": true})
    );
}

#[test]
fn the_allow_list_is_stetdocs_read_only_commands() {
    assert_eq!(
        profile().claude_options().allowed_tools(),
        [
            "Bash(stetdoc view:*)",
            "Bash(stetdoc inspect:*)",
            "Bash(stetdoc check:*)",
            "Bash(stetdoc lookup:*)",
            "Bash(stetdoc capabilities:*)",
        ]
    );
    assert_eq!(
        profile().claude_options().disallowed_tools(),
        Vec::<String>::new().as_slice()
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
    let mut keys = profile().codex_keys_without_index(Some(AgentKind::Codex));
    // One snapshot for every OS: macOS's Codex line (legal.rs checks it) is taken off first.
    if let Some(line) = clauseo_profile::instructions::codex_shell(cfg!(target_os = "macos")) {
        let text = keys["developer_instructions"].as_str().unwrap();
        let alone = text
            .strip_suffix(&format!("\n\n{line}"))
            .unwrap()
            .to_owned();
        keys.insert("developer_instructions".to_owned(), Value::String(alone));
    }
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
fn a_dotted_codex_key_is_dropped_since_codex_reads_it_as_a_path() {
    let mut keys = Map::new();
    for key in [
        "hooks.PreToolUse",
        "mcp_servers.evil",
        "features.apps",
        "model.x",
        "bypass_hook_trust",
    ] {
        keys.insert(key.to_owned(), json!(true));
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
    assert!(
        profile
            .codex_keys_without_index(Some(AgentKind::Claude))
            .is_empty()
    );
    assert!(profile.codex_keys_without_index(None).is_empty());
    assert!(profile.skill_files(None).is_none());
    // `browser` goes to Codex alone (`servers::browser_mcp`), and not on Windows: Claude keeps
    // the shell command.
    let codex = if cfg!(windows) {
        vec!["case-law"]
    } else {
        vec!["browser", "case-law"]
    };
    for (kind, want) in [
        (None, vec!["case-law"]),
        (Some(AgentKind::Claude), vec!["case-law"]),
        (Some(AgentKind::Codex), codex),
    ] {
        let names: Vec<String> = profile.servers(kind).into_iter().map(|s| s.name).collect();
        assert_eq!(names, want, "{kind:?}");
    }
    if !cfg!(windows) {
        assert_eq!(
            profile.servers(Some(AgentKind::Codex))[0],
            clauseo_profile::servers::browser(profile.paths())
        );
    }
}

#[test]
fn the_browser_entry_runs_the_guard_with_its_ports_and_variables() {
    // Codex's entry (`servers::browser_mcp`), as it goes out before the host fills it.
    let browser = clauseo_profile::servers::browser(profile().paths());
    assert_eq!(browser.command, abs("/app/clauseo-mcp"));
    let agent_browser = abs("/app/agent-browser").to_string_lossy().into_owned();
    let mut args = vec![
        "browser-guard",
        "--agent-browser",
        agent_browser.as_str(),
        "--cwd",
        "{browser_dir}",
        "--cdp-port",
        "{cdp_port}",
    ];
    if cfg!(windows) {
        args.extend(["--daemon-port", "{daemon_port}"]);
    }
    args.extend(["--", "mcp", "--tools", "core"]);
    assert_eq!(browser.args, args);
    let env: Vec<(&str, String)> = browser
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.replace('\\', "/")))
        .collect();
    let mut expected = vec![
        (
            "AGENT_BROWSER_CONFIG",
            "/data/browser/agent-browser.json".to_owned(),
        ),
        (
            "AGENT_BROWSER_EXECUTABLE_PATH",
            "{browser_executable}".to_owned(),
        ),
        ("AGENT_BROWSER_SESSION", "{session_id}".to_owned()),
    ];
    if cfg!(unix) {
        expected.push(("AGENT_BROWSER_SOCKET_DIR", "{socket_dir}".to_owned()));
    }
    expected.push(("AGENT_BROWSER_STREAM_PORT", "{stream_port}".to_owned()));
    assert_eq!(env, expected);
}

#[test]
fn every_token_the_profile_sends_is_one_the_host_is_told_to_fill() {
    let profile = profile();
    let mut texts = vec![profile.instructions()];
    let tokens: Vec<&str> = IN_INSTRUCTIONS.iter().chain(&IN_SERVERS).copied().collect();
    for server in profile.servers(Some(AgentKind::Codex)) {
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
    let folder = r#"/Users/a "b"/{session_id}/browser"#;
    let values = |folder| {
        [
            (BROWSER_DIR, folder),
            (SESSION_ID, "s1"),
            (PROXY_PORT, "1"),
            (BROWSER_EXECUTABLE, "/c"),
            (SOCKET_DIR, "/tmp/clauseo-ab-0123456789abcdef"),
            (STREAM_PORT, "61100"),
            (CDP_PORT, "61101"),
            (DAEMON_PORT, "61102"),
        ]
    };
    let filled = placeholders::fill("{browser_dir}", &IN_SERVERS, &values(folder)).unwrap();
    assert_eq!(filled, folder);
    let encoded = Value::String(filled).to_string();
    assert_eq!(encoded, r#""/Users/a \"b\"/{session_id}/browser""#);
    // A Windows folder, as the host fills it: native separators, no `\\?\` prefix, one
    // whole argument with its spaces.
    let windows = r"C:\Users\A B\AppData\Roaming\Clauseo\run\{session_id}\browser";
    assert_eq!(
        placeholders::fill("{browser_dir}", &IN_SERVERS, &values(windows)).unwrap(),
        windows
    );
    let instructions = placeholders::fill(
        &profile().instructions(),
        &IN_INSTRUCTIONS,
        &[
            (WORK_FOLDER, "/run/{matter_notes}/work"),
            (MATTER_NOTES, "/notes"),
            (BROWSER, ""),
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
        &[(MATTER_NOTES, "/notes"), (BROWSER, "")],
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "no value for the placeholder {work_folder}"
    );
}

#[test]
fn stetdocs_own_folder_goes_first_on_path() {
    assert_eq!(
        profile().path_prepend(),
        Some(abs("/app/stetdoc").as_path())
    );
}

#[test]
fn a_stetdoc_folder_shared_with_other_programs_goes_nowhere_on_path() {
    // Beside the browser sidecars (Tauri's externalBin), or beside the app itself.
    let beside_sidecars = paths_with("/app/clauseo", "/bin/stetdoc", "/bin").unwrap();
    assert_eq!(beside_sidecars.path_prepend(), None);
    let beside_app = paths_with("/app/clauseo", "/app/stetdoc-bin", "/sidecars").unwrap();
    assert_eq!(beside_app.path_prepend(), None);
    // In the root: never on PATH.
    assert_eq!(
        paths_with("/app/clauseo", "/stetdoc", "/app")
            .unwrap()
            .path_prepend(),
        None
    );
}

#[test]
fn a_relative_program_path_is_refused() {
    let relative = ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        abs("/app/clauseo"),
        PathBuf::from("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    );
    assert!(
        matches!(relative, Err(ProfileError::Path { .. })),
        "{relative:?}"
    );
    // A drive-relative Windows path is not absolute on any OS.
    let drive_relative = ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        abs("/app/clauseo"),
        PathBuf::from(r"C:stetdoc\stetdoc.exe"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    );
    assert!(drive_relative.is_err());
}

/// Windows paths: separators of both kinds and any case name the same folder.
#[cfg(windows)]
#[test]
fn a_windows_stetdoc_folder_matches_a_sidecar_folder_in_any_case_and_separator() {
    let paths = ProfilePaths::new(
        &Locations::under_root(Path::new(r"C:\data")),
        PathBuf::from(r"C:\Program Files\Clauseo\clauseo.exe"),
        PathBuf::from(r"c:/program files/clauseo/stetdoc.exe"),
        BrowserSidecars {
            clauseo_mcp: PathBuf::from(r"C:\Program Files\Clauseo\clauseo-mcp.exe"),
            agent_browser: PathBuf::from(r"C:\Program Files\Clauseo\agent-browser.exe"),
        },
    )
    .unwrap();
    assert_eq!(paths.path_prepend(), None);
    let own = ProfilePaths::new(
        &Locations::under_root(Path::new(r"C:\data")),
        PathBuf::from(r"C:\Program Files\Clauseo\clauseo.exe"),
        PathBuf::from(r"C:\Program Files\Clauseo\stetdoc\stetdoc.exe"),
        BrowserSidecars {
            clauseo_mcp: PathBuf::from(r"C:\Program Files\Clauseo\clauseo-mcp.exe"),
            agent_browser: PathBuf::from(r"C:\Program Files\Clauseo\agent-browser.exe"),
        },
    )
    .unwrap();
    assert_eq!(
        own.path_prepend(),
        Some(Path::new(r"C:\Program Files\Clauseo\stetdoc"))
    );
}

/// agent-browser's rules for Claude: the page subcommands allowed, the options that reach the
/// lawyer's own browser or swap Clauseo's settings denied, everything else asking. The profile
/// never sends them, with the browser or without: the host adds them per session.
#[test]
fn agent_browser_rules_are_the_hosts_per_session() {
    use clauseo_profile::claude::{
        AGENT_BROWSER_ALLOWED, AGENT_BROWSER_DENIED, agent_browser_allowed, agent_browser_denied,
    };
    let allowed: Vec<String> = agent_browser_allowed().collect();
    assert_eq!(allowed.len(), AGENT_BROWSER_ALLOWED.len());
    for rule in [
        "Bash(agent-browser open:*)",
        "Bash(agent-browser snapshot:*)",
        "Bash(agent-browser read:*)",
    ] {
        assert!(allowed.iter().any(|a| a == rule), "{rule}");
    }
    // eval runs page JavaScript: it asks each time.
    assert!(!allowed.iter().any(|a| a == "Bash(agent-browser eval:*)"));
    assert_eq!(
        clauseo_profile::claude::agent_browser_ask().collect::<Vec<_>>(),
        [
            "Bash(agent-browser wait*-f*)",
            "Bash(agent-browser get*cdp-url*)"
        ]
    );
    for asks in [
        "upload", "eval", "state", "cookies", "storage", "network", "set", "install",
    ] {
        assert!(
            !allowed.iter().any(|a| a.contains(&format!(" {asks}:"))),
            "{asks}"
        );
    }
    let denied: Vec<String> = agent_browser_denied().collect();
    assert_eq!(denied.len(), AGENT_BROWSER_DENIED.len() + 2);
    for rule in [
        "Bash(agent-browser *--profile*)",
        "Bash(agent-browser *--auto-connect*)",
        "Bash(agent-browser *--cdp*)",
        "Bash(agent-browser *--executable-path*)",
        "Bash(agent-browser *--headed*)",
        "Bash(agent-browser *--config*)",
        "Bash(agent-browser *--session*)",
        "Bash(agent-browser *--state*)",
        "Bash(agent-browser *--extension*)",
        "Bash(agent-browser state*)",
        "Bash(agent-browser connect*)",
    ] {
        assert!(denied.iter().any(|d| d == rule), "{rule}");
    }
    let (with, _) = profile().with_tool_skills(None, true);
    for options in [with.claude_options(), profile().claude_options()] {
        let meta = Value::Object(options.meta_fragment()).to_string();
        assert!(!meta.contains("agent-browser"), "{meta}");
    }
}

/// `{browser}` sits once in the tools part, and only the part a session with the browser gets
/// names `agent-browser`; the others say there is no browser, the not-installed one why.
#[test]
fn only_the_browser_part_names_agent_browser() {
    use clauseo_profile::instructions::{BrowserPart, browser_part, browser_part_for};
    assert_eq!(TOOLS.matches(BROWSER).count(), 1);
    let text = profile().instructions();
    assert!(!text.contains("agent-browser"));
    let fill = |part| {
        placeholders::fill(
            &text,
            &IN_INSTRUCTIONS,
            &[
                (WORK_FOLDER, "/w"),
                (MATTER_NOTES, "/n"),
                (BROWSER, browser_part(part)),
            ],
        )
        .unwrap()
    };
    let on = fill(BrowserPart::On);
    assert!(on.contains("Read web pages with your web fetch tool. For a page that needs a real"));
    for rule in [
        "`agent-browser`",
        "`AGENT_BROWSER_SESSION`",
        "`--headed`",
        "`state save`",
        "named in this conversation",
        "confidential matter text",
        "downloads folder",
        "`agent-browser close`",
    ] {
        assert!(on.contains(rule), "{rule}");
    }
    for part in [
        BrowserPart::Off,
        BrowserPart::NotInstalled,
        BrowserPart::ClaudeOnly,
    ] {
        let off = fill(part);
        assert!(!off.contains("agent-browser") && !off.contains("AGENT_BROWSER"));
        assert!(off.contains("You have no browser in this session"));
    }
    assert!(fill(BrowserPart::NotInstalled).contains("found no Google Chrome"));
    // A Codex session with the browser is told to use the `browser` MCP entry's tools, never the
    // shell command or its variables, and never to escalate; Claude never gets Codex's part.
    let codex_on = browser_part_for(BrowserPart::On, true);
    for rule in [
        "use your browser tools, the ones from the `browser` MCP server",
        "never a browser command in your shell",
        "confidential matter text",
        "close tool",
    ] {
        assert!(codex_on.contains(rule), "{rule}: {codex_on}");
    }
    for word in [
        "agent-browser",
        "AGENT_BROWSER",
        "sandbox_permissions",
        "require_escalated",
        "prefix_rule",
    ] {
        assert!(!codex_on.contains(word), "{word}: {codex_on}");
    }
    assert_eq!(
        browser_part_for(BrowserPart::On, false),
        browser_part(BrowserPart::On)
    );
    for part in [
        BrowserPart::Off,
        BrowserPart::NotInstalled,
        BrowserPart::ClaudeOnly,
    ] {
        assert_eq!(browser_part_for(part, true), browser_part(part));
        assert!(!browser_part_for(part, true).contains("agent-browser"));
    }
    assert!(word_count(browser_part(BrowserPart::NotInstalled)) <= 50);
}
