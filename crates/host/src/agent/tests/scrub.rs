//! No `CLAUDE_*`, `ANTHROPIC_*` or `IS_SANDBOX` from the app's own environment reaches an agent:
//! only the ones Clauseo sets itself (`agent::spec::SCRUBBED_PREFIXES`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_settings::AgentLaunch;

use super::absolute_program;
use crate::agent::launch_spec;

/// Whether `name` is one the scrub is about.
fn scrubbed(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("CLAUDE_") || upper.starts_with("ANTHROPIC_") || upper == "IS_SANDBOX"
}

/// A Claude launch as the host builds it: the home and temp folder forced, the sign-in's
/// variables in the agent's settings entry, the stored key and the firm plan's in the hook. With
/// a pile of loosening variables inherited, in any case, every name with those prefixes the spec
/// holds is one Clauseo set, with Clauseo's value; the rest of the inherited env stays.
#[test]
fn only_the_claude_and_anthropic_variables_clauseo_sets_reach_the_agent() {
    let home = Path::new("/data/agents/claude-acp/home");
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::from([
            ("ANTHROPIC_API_KEY".to_owned(), String::new()),
            (
                "CLAUDE_CONFIG_DIR".to_owned(),
                "/data/agents/claude-acp/home".to_owned(),
            ),
            (
                "CLAUDE_SECURESTORAGE_CONFIG_DIR".to_owned(),
                "/data/store".to_owned(),
            ),
        ]),
    };
    let mut forced = crate::agent::home::claude_launch_env(home);
    forced.extend(crate::agent::temp::session_env(
        Path::new("/data/run/s1"),
        Some(Path::new("/tmp/c1")),
        false,
    ));
    let hook = crate::agent::loads::launch_env(BTreeMap::from([
        (
            OsString::from("ANTHROPIC_API_KEY"),
            OsString::from("sk-stored"),
        ),
        (
            OsString::from("ANTHROPIC_BASE_URL"),
            OsString::from("https://firm.example"),
        ),
        (
            OsString::from("CLAUDE_CODE_USE_BEDROCK"),
            OsString::from("1"),
        ),
    ]));
    let planted = [
        "IS_SANDBOX",
        "is_sandbox",
        "CLAUDE_CODE_BUBBLEWRAP",
        "CLAUDE_CODE_SANDBOXED",
        "CLAUDE_BG_SESSION_PERMISSION_RULES",
        "CLAUDE_CODE_SESSION_KIND",
        "CLAUDE_CODE_AUTO_MODE_SERVER",
        "CLAUDE_CODE_DISABLE_PERMISSION_PROMPT_NOTIFY_HOOKS",
        "CLAUDE_CODE_DISABLE_HOOK_FORWARDING",
        "CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS",
        "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD",
        "CLAUDE_CODE_USE_COWORK_PLUGINS",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_REMOTE",
        "CLAUDE_CONFIG_DIR",
        "Claude_Code_Tmpdir",
        "CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "anthropic_model",
        "ENABLE_CLAUDEAI_MCP_SERVERS",
    ];
    let inherited = planted
        .iter()
        .map(|name| (OsString::from(name), OsString::from("planted")))
        .chain([
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            (OsString::from("HOME"), OsString::from("/home/ada")),
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (
                OsString::from("CLAUDEISH"),
                OsString::from("kept: no prefix"),
            ),
        ]);
    let spec = launch_spec(
        &agent,
        hook,
        forced,
        &[],
        inherited,
        std::env::temp_dir(),
        &[],
    )
    .unwrap();
    let ours: BTreeMap<&str, &str> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .filter(|(name, _)| scrubbed(name))
        .collect();
    let expected = BTreeMap::from([
        ("ANTHROPIC_API_KEY", "sk-stored"),
        ("ANTHROPIC_BASE_URL", "https://firm.example"),
        ("CLAUDE_CODE_TMPDIR", "/tmp/c1"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("CLAUDE_CONFIG_DIR", "/data/agents/claude-acp/home"),
        ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/data/store"),
    ]);
    assert_eq!(ours, expected);
    let env = spec.env();
    assert!(!env.values().any(|value| value == "planted"));
    assert_eq!(
        env.get(&OsString::from("ENABLE_CLAUDEAI_MCP_SERVERS"))
            .unwrap(),
        "false"
    );
    assert_eq!(
        env.get(&OsString::from("CLAUDEISH")).unwrap(),
        "kept: no prefix"
    );
    // unix-paths-ok: a planted settings or env value; it never reaches a path API
    assert_eq!(env.get(&OsString::from("HOME")).unwrap(), "/home/ada");
}

/// A Codex agent loses nothing it reads to the scrub: none of its names has those prefixes.
#[test]
fn codex_reads_none_of_the_scrubbed_names() {
    let names = crate::agent::codex::DROPPED_VARS.iter().chain(&[
        "CODEX_HOME",
        "CODEX_CONFIG",
        "CODEX_API_KEY",
        "OPENAI_BASE_URL",
    ]);
    for name in names {
        assert!(
            !crate::agent::spec::is_dropped_inherited(std::ffi::OsStr::new(name)),
            "{name}"
        );
    }
    assert_eq!(
        crate::agent::spec::SCRUBBED_PREFIXES,
        ["CLAUDE_", "ANTHROPIC_"]
    );
    assert_eq!(crate::agent::spec::SCRUBBED, ["IS_SANDBOX"]);
}
