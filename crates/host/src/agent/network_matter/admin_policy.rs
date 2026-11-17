//! Whether Claude Code has an administrator policy tier at this launch: then a `permissions.ask`
//! rule may have raised the ask for a read, and the host can't tell that ask from Claude Code's
//! own network-address one (claude-agent-acp 0.89.1 keeps `matchedAskRule` to itself), so it
//! answers none of them ([`super::reads`]).
//!
//! Mirrors host's `AdminPolicy` (branch `launch-never-weaker`, `offline/probe/settings/
//! policy_keys.rs`, cached as `Agent::admin_tier`): whichever of the two PRs lands second
//! reads that field instead and drops this. The documents, read once per launch on the prep's
//! thread (`offline/probe/settings.rs` `Reads::admin_docs`):
//! - in the managed folder (that of `managed-mcp.json`), `managed-settings.json`, then each
//!   `managed-settings.d/*.json` not starting `.`, in name order, at most 64;
//! - macOS managed preferences: `/Library/Managed Preferences/<login user>/
//!   com.anthropic.claudecode.plist`, then the computer's own;
//! - Windows: the `Settings` value of `HKLM\SOFTWARE\Policies\ClaudeCode` (`REG_SZ` or
//!   `REG_EXPAND_SZ`, the 64-bit view);
//! - Claude Code's server-managed cache, `remote-settings.json` in the agent's Claude home.
//!
//! A tier is present when any document has content, or is there but can't be read or parsed
//! (fail toward present). Claude Code re-reads managed settings mid-session (files within about
//! a second, managed preferences every 30 minutes); a policy an administrator adds during a
//! session leaves this launch-time answer stale until the next launch. Adding one takes admin
//! rights, so that is documented (`docs/firm-it.md`), not built for.

use serde_json::Value;

/// One file read: `None` when it is not there, `Err` when it is there but can't be read.
pub(crate) type FileRead = Result<Option<Vec<u8>>, ()>;

/// One administrator policy document, as read.
#[derive(Debug)]
pub(crate) enum AdminDoc {
    /// A JSON settings document: a file, or the registry's `Settings` text (an empty value
    /// read as not there, `os_policy.rs`).
    Json(FileRead),
    /// A managed-preferences property list (XML or binary).
    Plist(FileRead),
}

/// Whether an administrator policy tier was present at a launch. Not a `bool`: `Agent` holds
/// enough of those (`clippy::struct_excessive_bools`). Not computed yet is `Present`, the
/// default, so nothing is answered for an agent no one has read the tier of. Host's
/// `profile_loads::policy` defines the same type: the second to land keeps one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AdminTier {
    /// Some document has content, or could not be read or parsed; or not computed yet.
    #[default]
    Present,
    /// None is there, or each is empty.
    Absent,
}

impl AdminTier {
    /// The tier for a launch whose documents made one `present` ([`admin_policy_present`]).
    pub(crate) const fn of(present: bool) -> Self {
        if present { Self::Present } else { Self::Absent }
    }
}

/// Whether `docs` make an administrator policy tier present (above).
pub(crate) fn admin_policy_present(docs: &[AdminDoc]) -> bool {
    docs.iter().any(|doc| match doc {
        AdminDoc::Json(Ok(None)) | AdminDoc::Plist(Ok(None)) => false,
        AdminDoc::Json(Err(())) | AdminDoc::Plist(Err(())) => true,
        AdminDoc::Json(Ok(Some(bytes))) => json_has_content(bytes),
        AdminDoc::Plist(Ok(Some(bytes))) => plist_has_content(bytes),
    })
}

/// Whether a JSON settings document has content: anything but an empty object, an
/// unparsable document included.
fn json_has_content(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(map)) => !map.is_empty(),
        Ok(_) | Err(_) => true,
    }
}

/// Whether a property list has content: anything but an empty file or an empty dictionary,
/// an unparsable one included. An empty file is nothing, as Claude Code reads it.
fn plist_has_content(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    match plist::Value::from_reader(std::io::Cursor::new(bytes)) {
        Ok(plist::Value::Dictionary(dictionary)) => !dictionary.is_empty(),
        Ok(_) | Err(_) => true,
    }
}

#[cfg(test)]
mod tests;
