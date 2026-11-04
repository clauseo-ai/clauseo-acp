//! The helper's spec: its folder is the run folder, never the process's own, and a project
//! settings file in the run folder neither refuses the run nor moves the folder.

use std::collections::BTreeMap;

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
    let spec = prepare(&launch, &run_dir).unwrap();
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
