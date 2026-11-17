//! What Claude Code loads from Clauseo's data folder stays out of reach of Claude's own file
//! tools: a real launch's flag settings (the run's settings file, `agent::launch_settings`)
//! carry an `Edit` deny, which covers `Write`, `MultiEdit` and `NotebookEdit` too, for Claude's
//! settings, instructions, skills and plugins in its home, the built-in plugin and the run's
//! packs plugin. And the built-in plugin holds only what Clauseo ships when an agent starts.

use std::sync::Arc;

use serde_json::{Value, json};

use clauseo_protocol::AgentKind;

use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Wired, agent_lines, agents, codex_lines, settings, template, turn,
};
use crate::support::{Dir, Scenario};
use crate::write;

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

/// A new skill file planted in the built-in plugin after one launch is gone before the next
/// session starts, and that session's Codex skills index never names it.
#[test]
fn a_skill_planted_in_the_built_in_plugin_is_gone_before_the_next_session() {
    let dir = Dir::new("claude_folders_planted", &[]);
    let claude = template(
        &dir,
        "planted",
        &agent_lines("planted", "session/new", &json!({}), &[]),
    );
    let codex = template(
        &dir,
        "planted-codex",
        &codex_lines("planted-codex", &json!({})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let profile = Arc::new(crate::tools::profile(dir.data()));
    let hook = Hook {
        skills: profile.launch_writes(Some(AgentKind::Codex)),
        profile: Some(Arc::clone(&profile)),
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    let skills = dir
        .data()
        .join("profile")
        .join("claude-plugin")
        .join("skills");
    assert!(skills.is_dir(), "the first launch wrote the plugin");
    let planted = skills.join("zz-planted-skill").join("SKILL.md");
    write::write(
        &planted,
        b"---\nname: zz-planted-skill\ndescription: d\n---\nx\n",
    );
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    assert!(!planted.exists(), "the planted skill file is gone");
    assert!(!skills.join("zz-planted-skill").exists(), "and its folder");
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2, "both sessions started");
    let config = seen[1].codex_config().expect("a CODEX_CONFIG");
    let instructions = config["developer_instructions"].as_str().unwrap();
    assert!(
        instructions.contains(&profile.skills_index()),
        "{instructions}"
    );
    assert!(!instructions.contains("zz-planted-skill"), "{instructions}");
}

/// What Claude Code would load from Clauseo's home for it, planted there before a launch, is
/// gone when the agent starts: `skills`, `commands` and `agents` do not exist at all (Claude Code
/// watches them during a session only when they exist at its start), nor `CLAUDE.md`, `rules`,
/// `settings.local.json` and the rest; Claude Code's own `.claude.json` stays as it was.
#[test]
fn a_launch_starts_claude_with_nothing_loadable_in_its_home() {
    let dir = Dir::new("claude_home_cleared", &[]);
    settings(
        &dir,
        agents(&[(CLAUDE, &Scenario::Local("mcp-defaults"), json!({}))]),
        json!({}),
    );
    let home = dir.data().join("agents").join("claude-acp").join("home");
    let loaded = [
        "skills/zz-stray/SKILL.md",
        "commands/zz-stray.md",
        "agents/zz-stray.md",
        "rules/zz-stray.md",
        "agent-memory/zz-stray/MEMORY.md",
        "workflows/zz-stray.js",
        "plugins/zz-stray/plugin.json",
        "CLAUDE.md",
        "settings.local.json",
    ];
    for file in loaded.iter().chain(&[".claude.json"]) {
        let path = file.split('/').fold(home.clone(), |at, part| at.join(part));
        write::write(&path, b"{}");
    }
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
    assert_eq!(launcher.seen().len(), 1, "the session started");
    for file in loaded {
        let top = home.join(file.split('/').next().unwrap());
        assert!(
            std::fs::symlink_metadata(&top).is_err(),
            "{top:?} is still there"
        );
    }
    assert_eq!(std::fs::read(home.join(".claude.json")).unwrap(), b"{}");
}
