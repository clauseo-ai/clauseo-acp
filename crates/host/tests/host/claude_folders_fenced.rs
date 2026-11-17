//! What Claude Code loads from Clauseo's data folder stays out of reach of Claude's own file
//! tools: a real launch's flag settings (the run's settings file, `agent::launch_settings`)
//! carry an `Edit` deny, which covers `Write`, `MultiEdit` and `NotebookEdit` too, for Claude's
//! settings, instructions, skills and plugins in its home, the built-in plugin and the run's
//! packs plugin.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::profile_rig::{CLAUDE, Wired, agents, settings, turn};
use crate::support::{Dir, Scenario};

/// The `permissions.deny` rules of the first Claude launch's settings file in `dir`, and that
/// launch's run folder name.
fn launch_deny(dir: &Dir, launcher: &Wired) -> (Vec<String>, String) {
    let run = launcher.seen()[0].run.clone().expect("a run folder");
    let run_id = run.file_name().unwrap().to_str().unwrap().to_owned();
    let file = dir
        .data()
        .join("launch-settings")
        .join(format!("{run_id}.json"));
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let deny = written["permissions"]["deny"]
        .as_array()
        .unwrap_or_else(|| panic!("no deny rules: {written}"))
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    (deny, run_id)
}

#[test]
fn a_launch_denies_edits_to_what_claude_code_loads() {
    let dir = Dir::new("claude_folders_fenced", &[]);
    settings(
        &dir,
        agents(&[(CLAUDE, &Scenario::Local("mcp-defaults"), json!({}))]),
        json!({}),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    let (deny, run_id) = launch_deny(&dir, &launcher);
    host.shutdown();
    // The rules name the data folder in its rule form (`//…`, `/c/…` on Windows): each place
    // is matched from the data folder's own name on.
    let data_name = dir.data().file_name().unwrap().to_str().unwrap().to_owned();
    let home = "agents/claude-acp/home";
    for place in [
        format!("{home}/settings.json"),
        format!("{home}/settings.local.json"),
        format!("{home}/CLAUDE.md"),
        format!("{home}/skills/**"),
        format!("{home}/plugins/**"),
        "profile/claude-plugin/**".to_owned(),
        format!("run/{run_id}/skill-packs/**"),
    ] {
        let tail = format!("/{data_name}/{place})");
        assert!(
            deny.iter()
                .any(|rule| rule.starts_with("Edit(//") && rule.ends_with(&tail)),
            "no Edit deny for {place}: {deny:?}"
        );
    }
}
