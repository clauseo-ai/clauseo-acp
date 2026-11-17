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

/// Every read keeps Claude Code's scheduler off and adds no repository status, whatever the
/// entry or the inherited environment say, as the host's Claude launches do.
#[test]
fn every_read_keeps_the_scheduler_off() {
    let scratch = Scratch::new("claude-prepare-cron");
    let script = scratch.file(CLAUDE_SCRIPT, "");
    scratch.file(CLAUDE_SDK, SDK_PACKAGE);
    let launch = AgentLaunch {
        command: node(),
        args: vec![script.to_string_lossy().into_owned()],
        env: BTreeMap::from([("claude_code_disable_cron".to_owned(), "0".to_owned())]),
    };
    let run_dir = scratch.path("data/limits/run");
    let inherited = [(
        OsString::from("CLAUDE_CODE_DISABLE_CRON"),
        OsString::from(""),
    )];
    let spec = prepare(&launch, &run_dir, None, inherited.into_iter()).unwrap();
    let named: Vec<_> = spec
        .env()
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("CLAUDE_CODE_DISABLE_CRON"))
        .collect();
    assert_eq!(
        named,
        [(
            &OsString::from("CLAUDE_CODE_DISABLE_CRON"),
            &OsString::from("1")
        )]
    );
    assert_eq!(
        spec.env()
            .get(OsStr::new("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS")),
        Some(&OsString::from("1"))
    );
    for (name, value) in [
        ("CLAUDE_CODE_HARBOR_KITE", "0"),
        ("CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL", "1"),
    ] {
        assert_eq!(
            spec.env().get(OsStr::new(name)),
            Some(&OsString::from(value))
        );
    }
}
