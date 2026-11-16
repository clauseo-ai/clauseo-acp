//! "Always for this matter" on a Claude permission request: the rules Clauseo keeps for a
//! matter, as `HostQuery::KeptPermissions` lists them and `HostCommand::RemoveKeptPermission`
//! takes one back. Never logged and in no stream: a query, its answer and a command carry them.
//!
//! - A rule is Claude Code permission rule text (`Bash(npm test)`, `WebFetch(domain:host)`, an
//!   MCP tool's name, `Edit(//<path>)`, `Read(//<path>)`). The host reads it into a
//!   [`KeptKind`] and a subject, so a window shows it without parsing rule syntax, and sends the
//!   exact `rule` back to remove it.
//! - [`KeptKind`] is closed: the desktop app ships with the host (0002's freeze rules), so a new
//!   kind is additive.

use serde::{Deserialize, Serialize};

use crate::{Error, Measure};

/// Most bytes in a kept rule: the host keeps none longer.
pub const MAX_KEPT_RULE_BYTES: usize = 2_000;

/// What a kept rule allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum KeptKind {
    /// Running one exact command (`Bash`, `PowerShell`, or Codex's `Run`); the subject is the
    /// command.
    Run,
    /// Changing files inside the matter folder (`Edit(*)`, which covers every file-changing
    /// tool); the subject is empty.
    Edit,
    /// Reading one file; the subject is the file.
    Read,
    /// Fetching any page on one site (`WebFetch`); the subject is the host name.
    Site,
    /// One tool of an MCP server; the subject is the tool's name, `server` the server's.
    Tool,
}

/// One rule Clauseo keeps for a matter, as its list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct KeptPermission {
    /// The rule's exact text: what `RemoveKeptPermission` sends back.
    pub rule: String,
    /// What it allows.
    pub kind: KeptKind,
    /// The command, file, host name or tool name. A file inside the matter folder is relative
    /// to it, with `/` between names; one outside it is the whole path, as this system spells
    /// it.
    pub subject: String,
    /// The MCP server, for `Tool`; left out for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// For a kept Codex command: the working folder relative to the matter folder, `/`-separated;
    /// none when it runs in the matter folder itself, and for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// `RemoveKeptPermission`'s rule: 1 to [`MAX_KEPT_RULE_BYTES`] bytes with no control character;
/// whether the matter keeps it is state.
pub(crate) fn check_rule(rule: &str) -> Result<(), Error> {
    const KIND: &str = "RemoveKeptPermission";
    if rule.len() > MAX_KEPT_RULE_BYTES {
        return Err(Error::TooLarge {
            kind: KIND,
            what: Measure::RuleBytes,
            size: rule.len(),
            max: MAX_KEPT_RULE_BYTES,
        });
    }
    if rule.is_empty() || rule.chars().any(char::is_control) {
        return Err(Error::Format {
            kind: KIND,
            rule: "a rule is not empty and holds no control character",
        });
    }
    Ok(())
}
