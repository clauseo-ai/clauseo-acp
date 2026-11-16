//! The real CLIs' writes into their config folders on any run, mimicked so an engine test that
//! snapshots the lawyer's home catches a run Clauseo starts on the lawyer's own folders.
//!
//! - Claude Code's `auth status` (2.x, measured with claude-agent-acp 0.89.1): the first time a
//!   CLI version meets a config folder it rewrites the global config file and writes a backup
//!   of it into `backups/`, and creates both in an empty folder. The global config is
//!   `$CLAUDE_CONFIG_DIR/.claude.json` when that is set and not empty, else `$HOME/.claude.json`;
//!   the backups go to `$CLAUDE_CONFIG_DIR/backups`, else `$HOME/.claude/backups`.
//! - Codex 0.162.1's `login status` and `app-server`: each run creates
//!   `<codex home>/tmp/arg0/codex-arg0<random>/` holding a `.lock` and its helper links, even
//!   when signed out. The fake writes the `.lock` only.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::write;

/// The key the fake adds to the global config once it has met it: the version stamp the real CLI
/// keeps there, so a second run writes nothing more.
const MET: &str = "fakeAgentMet";

/// Claude Code's first meeting with its config folder: see the module docs. `config` is
/// `CLAUDE_CONFIG_DIR`, `home` the user's home folder.
pub(crate) fn claude_status(config: Option<OsString>, home: &Path) -> io::Result<()> {
    let (file, backups) = match config.filter(|config| !config.is_empty()) {
        Some(config) => {
            let config = PathBuf::from(config);
            (config.join(".claude.json"), config.join("backups"))
        }
        None => (
            home.join(".claude.json"),
            home.join(".claude").join("backups"),
        ),
    };
    let old = std::fs::read_to_string(&file).ok();
    let mut global = old
        .as_deref()
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if global.get(MET).is_some() {
        return Ok(());
    }
    write::create_dir_all(&backups)?;
    let backup = format!(".claude.json.backup.{}", std::process::id());
    write::write(backups.join(backup), old.unwrap_or_default())?;
    global[MET] = json!(true);
    if let Some(parent) = file.parent() {
        write::create_dir_all(parent)?;
    }
    write::write(
        &file,
        format!(
            "{global}
"
        ),
    )
}

/// Codex's helper folder, made by each `login status` and `app-server` run: see the module
/// docs. `codex_home` is `CODEX_HOME`, else `$HOME/.codex`.
pub(crate) fn codex_run(codex_home: &Path) -> io::Result<()> {
    let folder = codex_home
        .join("tmp")
        .join("arg0")
        .join(format!("codex-arg0{}", std::process::id()));
    write::create_dir_all(&folder)?;
    write::write(folder.join(".lock"), "")
}
