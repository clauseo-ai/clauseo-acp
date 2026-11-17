//! The helper's spec: its folder is the run folder, never the process's own, and a project
//! settings file in the run folder neither refuses the run nor moves the folder; Clauseo's own
//! Claude home, when given, is the read's `CLAUDE_CONFIG_DIR` whatever was inherited.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clauseo_settings::AgentLaunch;

use crate::readers::claude::prepare;
use crate::write::scratch::Scratch;

const CLAUDE_SCRIPT: &str =
    "install/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
const CLAUDE_SDK: &str = "install/node_modules/@anthropic-ai/claude-agent-sdk/package.json";
const SDK_PACKAGE: &str = r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"0.3.293"}"#;
/// A project settings file that would move the account or provider Claude Code reads.
const VERTEX_SETTINGS: &str = r#"{"env":{"CLAUDE_CODE_USE_VERTEX":"1","ANTHROPIC_VERTEX_PROJECT_ID":"clauseo-g16-not-a-project","CLOUD_ML_REGION":"us-east5"}}"#;

fn node() -> String {
    if cfg!(windows) {
        r"C:\node\node.exe".to_owned()
    } else {
        "/usr/local/bin/node".to_owned()
    }
}

#[test]
fn the_helper_runs_in_the_made_run_folder_not_the_process_folder() {
    let scratch = Scratch::new("claude-prepare-cwd");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let launch = AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: BTreeMap::new(),
    };
    let run_dir = scratch.path("data/limits/run");
    scratch.file("data/limits/run/.claude/settings.json", VERTEX_SETTINGS);
    let spec = prepare(&launch, &run_dir, None, std::env::vars_os()).unwrap();
    assert_eq!(spec.cwd(), run_dir);
    assert!(run_dir.is_dir());
    let process_dir = std::env::current_dir().unwrap();
    assert_ne!(
        spec.cwd(),
        process_dir,
        "the helper must not run in the app's folder"
    );
    assert!(!process_dir.starts_with(&run_dir));
}

#[test]
fn the_claude_home_is_forced_over_an_inherited_and_a_settings_config_dir() {
    let scratch = Scratch::new("claude-prepare-home");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let home = scratch.path("data/agents/claude-acp/home");
    let launch = AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: BTreeMap::from([("claude_config_dir".to_owned(), "/settings".to_owned())]),
    };
    let run_dir = scratch.path("data/limits/run");
    let inherited = || {
        [
            (
                OsString::from("CLAUDE_CONFIG_DIR"),
                OsString::from("/elsewhere"),
            ),
            (OsString::from("PATH"), OsString::from("/usr/bin")),
        ]
        .into_iter()
    };
    let spec = prepare(&launch, &run_dir, Some(&home), inherited()).unwrap();
    let named: Vec<_> = spec
        .env()
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR"))
        .collect();
    assert_eq!(
        named,
        [(&OsString::from("CLAUDE_CONFIG_DIR"), &home.into_os_string())]
    );
    assert_eq!(
        spec.env().get(OsStr::new("PATH")),
        Some(&OsString::from("/usr/bin"))
    );
    // With no home, the entry's variable stands over the inherited one, as before.
    let spec = prepare(&launch, &run_dir, None, inherited()).unwrap();
    assert_eq!(
        spec.env().get(OsStr::new("claude_config_dir")),
        Some(&OsString::from("/settings"))
    );
    assert!(spec.env().get(OsStr::new("CLAUDE_CONFIG_DIR")).is_none());
}

/// A base URL inherited from the app's environment or set in the entry's env, with no key, stops
/// the read before anything runs, as Claude Code would send its own sign-in there; a key in
/// either lets it go.
#[test]
fn a_base_url_from_the_app_or_the_entry_with_no_key_runs_nothing() {
    let scratch = Scratch::new("claude-prepare-base-url");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let home = scratch.path("data/agents/claude-acp/home");
    let run_dir = scratch.path("data/limits/run");
    let url = ("ANTHROPIC_BASE_URL", "https://gateway.firm.example");
    let launch = |env: &[(&str, &str)]| AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    };
    let inherited = |vars: &[(&str, &str)]| {
        vars.iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect::<Vec<_>>()
            .into_iter()
    };
    let key = ("ANTHROPIC_API_KEY", "k");
    let read = |entry: &[(&str, &str)], app: &[(&str, &str)]| {
        prepare(&launch(entry), &run_dir, Some(&home), inherited(app))
    };
    assert!(read(&[], &[url]).is_err(), "inherited");
    assert!(read(&[url], &[]).is_err(), "the entry's");
    assert!(read(&[url, key], &[]).is_ok());
    assert!(read(&[], &[url, key]).is_ok());
}

/// A Claude Code settings source that cannot be read, with no key, stops the read for its own
/// reason, as where Claude would connect is unknown; an endpoint in the env keeps that reason,
/// and a key lets it go.
#[test]
fn an_unreadable_settings_source_with_no_key_runs_nothing_for_its_own_reason() {
    let scratch = Scratch::new("claude-prepare-unreadable");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    scratch.file("data/agents/claude-acp/home/.claude.json", "{ not json");
    let home = scratch.path("data/agents/claude-acp/home");
    let run_dir = scratch.path("data/limits/run");
    let read = |env: &[(&str, &str)]| {
        let launch = AgentLaunch {
            command: node(),
            args: vec![script.to_string_lossy().into_owned()],
            env: env
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        };
        prepare(&launch, &run_dir, Some(&home), std::iter::empty())
    };
    let Err(unreadable) = read(&[]) else {
        panic!("an unreadable source with no key runs");
    };
    assert!(
        unreadable.contains("settings could not be read"),
        "{unreadable}"
    );
    let url = [("ANTHROPIC_BASE_URL", "https://gateway.firm.example")];
    let Err(endpoint) = read(&url) else {
        panic!("a custom endpoint with no key runs");
    };
    assert!(endpoint.contains("custom endpoint"), "{endpoint}");
    assert!(read(&[("ANTHROPIC_API_KEY", "k")]).is_ok());
}
