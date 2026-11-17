//! Claude Code's `ConfigChange` hook entry in a session's flag settings
//! ([`config_change_entry`]): the guard program in its config-change mode
//! (`pre_write_guard::config_change`), so a running session reloads no user, project or local
//! settings file and no skills folder changed after its start. A firm's managed settings are
//! not matched.

use std::path::Path;

use serde_json::Value;

use super::GUARD_TIMEOUT_SECONDS;
use crate::pre_write_guard::config_change;

/// The hooks key of Claude Code's event for a settings file or skills folder that changed during
/// a session.
pub(crate) const CONFIG_CHANGE: &str = "ConfigChange";

/// The `ConfigChange` entry that runs `program` with the guard's flag and
/// [`config_change::FLAG`] for each of [`config_change::MATCHER`]'s sources, in the exec form
/// the pre-write guard's entry uses (`super::guard_entry`: `command` and `args`, no shell).
/// `None` when `program` is no UTF-8 string, as that entry is.
pub(crate) fn config_change_entry(program: &Path) -> Option<Value> {
    let command = program.to_str()?;
    Some(serde_json::json!({
        "matcher": config_change::MATCHER,
        "hooks": [{
            "type": "command",
            "command": command,
            "args": [crate::pre_write_guard::FLAG, config_change::FLAG],
            "timeout": GUARD_TIMEOUT_SECONDS,
        }],
    }))
}
