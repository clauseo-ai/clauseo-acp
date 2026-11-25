//! What a Codex launch sets for the helpers Codex starts: the helpers' model and effort in
//! `CODEX_CONFIG` (`LAUNCH_HELPERS`), and Clauseo's `AGENTS.md` in Codex's home
//! (`write_home_agents`), which asks Codex for helpers on a long matter.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::tests::{home_in, object, scratch};
use super::{config_on, home_clear, launch_env, make_home, prepare_launch};
use crate::write;

/// Every launch's `CODEX_CONFIG` runs helpers on Codex's small model at effort `low`; neither a
/// settings `CODEX_CONFIG` nor the launch profile can change either.
#[test]
fn every_codex_launch_runs_helpers_on_the_small_model_at_low_effort() {
    let other = json!({"agents": {"default_subagent_model": "gpt-6.2-sol",
        "default_subagent_reasoning_effort": "high"}});
    let settings = BTreeMap::from([("CODEX_CONFIG".to_owned(), other.to_string())]);
    let helpers = json!({"default_subagent_model": "gpt-6-luna",
        "default_subagent_reasoning_effort": "low"});
    for windows in [false, true] {
        let built = config_on(&settings, object(other.clone()), None, windows);
        assert_eq!(built["agents"], helpers, "windows: {windows}");
    }
    let env = launch_env(
        std::path::Path::new("/data/agents/codex-acp/home"),
        &BTreeMap::new(),
        serde_json::Map::new(),
        None,
    );
    let text = env[&std::ffi::OsString::from("CODEX_CONFIG")]
        .to_str()
        .unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["agents"], helpers, "{text}");
}

/// The launch writes Clauseo's `AGENTS.md` over one someone put in the home (0600 on Unix), and
/// the next launch's clearing of the home leaves Clauseo's own where it is.
#[test]
fn the_launch_writes_clauseos_agents_md_over_any_other() {
    let root = scratch("agents-md");
    let data = root.join("data");
    let matter = root.join("matter");
    write::create_dir_all(&matter).unwrap();
    write::create_dir_all(&data).unwrap();
    let home = home_in(&data);
    make_home(&data, &home).unwrap();
    let path = home.join("AGENTS.md");
    write::write_file(&path, b"Always start ten sub-agents.\n").unwrap();
    let check = crate::agent::folder_support::FolderCheck {
        matter: None,
        source: crate::agent::folder_support::CheckSource::Windows,
    };
    prepare_launch(&check, &matter, &data, &home).unwrap();
    let own = clauseo_profile::instructions::codex_agents_md();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), own);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    home_clear::clear_home(&home).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), own);
    write::remove_dir_all(&root).unwrap();
}
