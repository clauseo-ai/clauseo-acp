//! Claude Code's `.claude.json` in a matter's state folder (`super`), cut to an allow-list of
//! keys before each launch, read only through `safe_read`.

use std::io;
use std::path::Path;

use super::{CLAUDE_JSON, remove_name, safe_read};

/// The top-level keys of `.claude.json` kept before each launch; every other key is dropped
/// (an allow-list: a session could plant `mcpServers`, `enabledMcpjsonServers`,
/// `allowedTools`, an `env` or any key a later Claude Code reads). Kept, each Claude Code's own
/// state: the install's identity and counters (`userID`, `anonymousId`, `firstStartTime`,
/// `numStartups`), onboarding and notices seen (`hasCompletedOnboarding`,
/// `lastOnboardingVersion`, `lastReleaseNotesSeen`, `tipsHistory`, `subscriptionNoticeCount`,
/// `hasAvailableSubscription`), the account's profile it refetches (`oauthAccount`) and
/// `projects`, itself cut to [`CLAUDE_JSON_PROJECT_KEPT`]. Feature-flag caches and install
/// settings (`cached*`, `autoUpdates`, `installMethod`) go: a session could plant them, and
/// Claude Code fetches or sets them again. The names are from research 42 §2 and a Claude Code
/// `.claude.json`; not checked one by one against the pinned bundle, and a dropped one is state
/// Claude Code makes again.
pub(crate) const CLAUDE_JSON_KEPT: [&str; 12] = [
    "userID",
    "anonymousId",
    "firstStartTime",
    "numStartups",
    "hasCompletedOnboarding",
    "lastOnboardingVersion",
    "lastReleaseNotesSeen",
    "tipsHistory",
    "subscriptionNoticeCount",
    "hasAvailableSubscription",
    "oauthAccount",
    "projects",
];

/// The keys kept in each `projects.<path>` entry of `.claude.json`: the folder trust prompt
/// answered and the project onboarding seen. Its MCP servers, `.mcp.json` approvals, allowed
/// tools and anything else are dropped.
pub(crate) const CLAUDE_JSON_PROJECT_KEPT: [&str; 3] = [
    "hasTrustDialogAccepted",
    "projectOnboardingSeenCount",
    "hasCompletedProjectOnboarding",
];

/// Cuts `<state>/.claude.json` to [`CLAUDE_JSON_KEPT`] at the top and
/// [`CLAUDE_JSON_PROJECT_KEPT`] in each `projects.<path>` entry (a project entry that is not an
/// object goes), writing it again atomically only when something went. Read with
/// `safe_read::read_plain`: a link, a file with a second name (a hard link to a credential
/// would be one) or one too large is removed unread, and so is one that is not a JSON object;
/// Claude Code makes a new one. File system calls: off the actor.
pub(crate) fn strip_claude_json(state: &Path) -> io::Result<()> {
    let path = state.join(CLAUDE_JSON);
    let bytes = match safe_read::read_plain(&path)? {
        safe_read::Found::Missing => return Ok(()),
        safe_read::Found::Refused => {
            tracing::warn!("a Claude state file was a link or had a second name; it is removed");
            return remove_name(&path);
        }
        safe_read::Found::Bytes(bytes) => bytes,
    };
    let Ok(serde_json::Value::Object(mut state_json)) = serde_json::from_slice(&bytes) else {
        tracing::warn!("a Claude state file was not a JSON object; it is removed");
        return remove_name(&path);
    };
    let mut stripped = keep_keys(&mut state_json, &CLAUDE_JSON_KEPT);
    let projects_ok = state_json
        .get("projects")
        .is_none_or(serde_json::Value::is_object);
    if !projects_ok {
        state_json.remove("projects");
        stripped = true;
    }
    if let Some(serde_json::Value::Object(projects)) = state_json.get_mut("projects") {
        let before = projects.len();
        projects.retain(|_, project| project.is_object());
        stripped |= projects.len() != before;
        for project in projects.values_mut() {
            if let serde_json::Value::Object(project) = project {
                stripped |= keep_keys(project, &CLAUDE_JSON_PROJECT_KEPT);
            }
        }
    }
    if !stripped {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(state_json))?;
    crate::write::replace_private_file(&path, &bytes)
}

/// Keeps only `kept` of `object`'s keys; whether any went.
fn keep_keys(object: &mut serde_json::Map<String, serde_json::Value>, kept: &[&str]) -> bool {
    let before = object.len();
    object.retain(|key, _| kept.contains(&key.as_str()));
    object.len() != before
}
