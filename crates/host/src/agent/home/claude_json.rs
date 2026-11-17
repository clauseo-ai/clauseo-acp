//! Claude Code's global config file in Clauseo's Claude home, `.claude.json`: its own state (the
//! account it signed in to, a Console key from `auth login --console`, caches, counters and
//! migrations), and keys it reads as configuration ([`CONFIG_KEYS`]), some of which change a
//! session: `env` is copied into its environment at its start, `mcpServers` and the per-folder
//! `projects` entries hold MCP servers, and the per-folder entries' MCP approvals are copied
//! into the matter's local settings. Clauseo writes none of them, so before each Claude launch
//! and each sign-in, status check and logout they go ([`strip_config`]) and the state stays.

use std::path::Path;

use serde_json::Value;

/// The file's name in the home (Claude Code 2.1.293: `.claude${suffix}.json` in
/// `CLAUDE_CONFIG_DIR`, the suffix empty in its shipped build).
pub(crate) const CLAUDE_JSON: &str = ".claude.json";

/// The most a file is read: a larger one is set aside whole.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The top-level keys of `.claude.json` that Claude Code 2.1.293 reads as configuration, each
/// removed: the global-config keys its `config` command sets (its list beside the config's
/// defaults, `apiKeyHelper` to `workflowSizeGuideline`), then `mcpServers` (user-scope MCP
/// servers), `projects` (per-folder MCP servers, MCP approvals, allowed tools and trust),
/// `bypassPermissionsModeAccepted` (copied into the user settings at its start) and
/// `remoteHomeSettingsMode` (its cloud settings forwarding choice).
pub(crate) const CONFIG_KEYS: [&str; 51] = [
    "apiKeyHelper",
    "installMethod",
    "autoUpdates",
    "autoUpdatesProtectedForNative",
    "theme",
    "verbose",
    "preferredNotifChannel",
    "shiftEnterKeyBindingInstalled",
    "editorMode",
    "hasUsedBackslashReturn",
    "autoCompactEnabled",
    "autoScrollEnabled",
    "showTurnDuration",
    "externalEditorContext",
    "showMessageTimestamps",
    "diffTool",
    "env",
    "tipsHistory",
    "todoFeatureEnabled",
    "showExpandedTodos",
    "briefTranscript",
    "diffSidebarOpen",
    "messageIdleNotifThresholdMs",
    "autoConnectIde",
    "autoInstallIdeExtension",
    "fileCheckpointingEnabled",
    "terminalProgressBarEnabled",
    "showStatusInTerminalTab",
    "taskCompleteNotifEnabled",
    "inputNeededNotifEnabled",
    "agentPushNotifEnabled",
    "respectGitignore",
    "claudeInChromeDefaultEnabled",
    "hasCompletedClaudeInChromeOnboarding",
    "lspRecommendationDisabled",
    "lspRecommendationNeverPlugins",
    "lspRecommendationIgnoredCount",
    "copyFullResponse",
    "copyOnSelect",
    "leftArrowOpensAgents",
    "defaultToAgentsView",
    "prStatusFooterEnabled",
    "remoteControlAtStartup",
    "autoUploadSessions",
    "autoAddRemoteControlDaemonWorker",
    "remoteDialogSeen",
    "workflowSizeGuideline",
    "mcpServers",
    "projects",
    "bypassPermissionsModeAccepted",
    "remoteHomeSettingsMode",
];

/// Removes each of [`CONFIG_KEYS`] from `<home>/.claude.json` and keeps every other key as it
/// was, unknown ones included, replacing the file atomically; nothing is written when none is
/// there, and nothing there is fine. What Claude Code would not read as its config is set aside
/// and removed whole ([`super::clear::set_aside`]): a link (never followed), a folder, a file
/// too large or whose JSON is not an object. A file that cannot be replaced is set aside and
/// written fresh. Returns whether the file changed; an error is the caller's to refuse on. File
/// system calls: off the actor.
pub(crate) fn strip_config(home: &Path) -> std::io::Result<bool> {
    let path = home.join(CLAUDE_JSON);
    let meta = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        other => other?,
    };
    let held = (meta.is_file() && meta.len() <= MAX_BYTES)
        .then(|| std::fs::read(&path))
        .transpose()?;
    let parsed = held.map(|bytes| serde_json::from_slice::<Value>(&bytes));
    let Some(Ok(Value::Object(mut config))) = parsed else {
        drop_aside(home)?;
        return Ok(true);
    };
    let before = config.len();
    config.retain(|key, _| !CONFIG_KEYS.contains(&key.as_str()));
    if config.len() == before {
        return Ok(false);
    }
    tracing::warn!(
        removed = before.saturating_sub(config.len()),
        "removed configuration from Claude Code's global config"
    );
    let bytes = serde_json::to_vec_pretty(&Value::Object(config))?;
    match crate::write::replace_private_file(&path, &bytes) {
        Ok(()) => Ok(true),
        Err(first) => {
            drop_aside(home).map_err(|_aside| first)?;
            crate::write::write_new_private_file(&path, &bytes)?;
            Ok(true)
        }
    }
}

/// Sets `.claude.json` aside and removes it, best-effort; an error only when it stays at its
/// name.
fn drop_aside(home: &Path) -> std::io::Result<()> {
    if let Some(aside) = super::clear::set_aside(home, CLAUDE_JSON)?
        && super::clear::remove_entry(&aside).is_err()
    {
        tracing::warn!("Claude Code's old global config stays aside for now");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
