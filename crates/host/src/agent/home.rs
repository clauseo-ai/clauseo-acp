//! Clauseo's own home for each agent with a kind: `Locations::agent_home` of its id,
//! `<data_dir>/agents/<id>/home`, made at each launch (`actor/turn/launch_checks.rs`) and forced
//! over any inherited, settings or hook value. Codex gets it as `CODEX_HOME` (`agent/codex.rs`),
//! Claude Code as `CLAUDE_CONFIG_DIR` ([`claude_launch_env`]), so neither reads the lawyer's own
//! `~/.codex` or `~/.claude`: their allow rules, hooks and skills stay out of Clauseo's sessions
//! (decision record 0048). The profile's Codex skills go in the same folder
//! (`clauseo_profile::paths::codex_skills`, from [`AgentKind::id`]), and Claude's plan notes are
//! marked against it (`agent/plan_note.rs` reads the launch's merged environment, which holds
//! this variable by then).
//!
//! [`AgentKind::id`]: clauseo_protocol::AgentKind::id

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

/// The variable Claude Code takes its config folder from: its settings, its login, its plan
/// notes (`<CLAUDE_CONFIG_DIR>/plans`) and its own skills and hooks.
pub(crate) const CLAUDE_CONFIG_VAR: &str = "CLAUDE_CONFIG_DIR";

/// What the host forces on a Claude agent's launch: `home` as `CLAUDE_CONFIG_DIR`.
pub(crate) fn claude_launch_env(home: &Path) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([(
        OsString::from(CLAUDE_CONFIG_VAR),
        home.as_os_str().to_owned(),
    )])
}
