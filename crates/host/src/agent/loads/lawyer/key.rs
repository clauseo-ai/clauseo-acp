//! What a session's lawyer servers were made from (Settings › Tools), so a change to them can
//! reach a live agent (`actor/turn/idle/tools.rs`).
//!
//! - [`ToolsKey::of`] is taken on the actor when a launch's prep starts, beside the keychain
//!   reads ([`super::Reads::of`]): the enabled servers' entries, and each one's keychain
//!   revision (`McpOnActor::secret_revs`, bumped when a save writes one of its values, since its
//!   entry then still reads `{"keychain": true}`). Nothing in the Safe workspace.
//! - [`ToolsKey::same_for`] compares two keys as one agent sees them: a remote server counts
//!   only for an agent that takes remote servers (`mcpCapabilities.http`).
//! - `rev` is the actor's Tools revision (`McpOnActor::tools_rev`) at the time: a key taken at
//!   the current revision needs no comparison.
//! - The key is one matter's (decision record 0040): only the servers on in it, the skill packs
//!   on in it with their recorded hashes, and Clauseo's own tools switched off in it, so a
//!   matter's switch or an item's scope changing reaches that matter's live agents only.
//! - `Debug` shows counts only: an entry may hold a URL or a plain value.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use clauseo_settings::{McpServerLaunch, McpTransport};

use crate::actor::addon_rules::Placed;

/// The lawyer's servers a launch was made from, by name, with their keychain revisions.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolsKey {
    /// The actor's Tools revision when it was taken.
    pub(crate) rev: u64,
    servers: BTreeMap<String, (McpServerLaunch, u64)>,
    /// The skill packs on in the matter, with their recorded hashes.
    skills: BTreeMap<String, String>,
    /// Clauseo's own tools switched off in the matter.
    builtin_off: BTreeSet<&'static str>,
}

impl fmt::Debug for ToolsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolsKey")
            .field("rev", &self.rev)
            .field("servers", &self.servers.len())
            .field("skills", &self.skills.len())
            .field("builtin_off", &self.builtin_off)
            .finish()
    }
}

impl ToolsKey {
    /// The key of what `placed` puts in its matter now, at Tools revision `rev`, with the
    /// keychain revisions `secret_revs`; no server in the Safe workspace (`safe_workspace`).
    pub(crate) fn of(
        placed: &Placed<'_>,
        safe_workspace: bool,
        secret_revs: &BTreeMap<String, u64>,
        rev: u64,
    ) -> Self {
        let servers = if safe_workspace {
            BTreeMap::new()
        } else {
            placed
                .servers()
                .map(|(name, server)| {
                    let secrets = secret_revs.get(name).copied().unwrap_or_default();
                    (name.to_owned(), (server.clone(), secrets))
                })
                .collect()
        };
        Self {
            rev,
            servers,
            skills: placed.skills(),
            builtin_off: placed.builtin_off(),
        }
    }

    /// Whether an agent that takes remote servers when `http` gets the same servers, skills and
    /// Clauseo tools from both.
    pub(crate) fn same_for(&self, other: &Self, http: bool) -> bool {
        self.seen(http).eq(other.seen(http))
            && self.skills == other.skills
            && self.builtin_off == other.builtin_off
    }

    /// The servers an agent that takes remote servers when `http` gets, in name order.
    fn seen(&self, http: bool) -> impl Iterator<Item = (&String, &(McpServerLaunch, u64))> {
        self.servers.iter().filter(move |(_, (server, _))| {
            http || !matches!(server.transport, McpTransport::Remote { .. })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(servers: &[(&str, &str, bool)], revs: &[(&str, u64)]) -> ToolsKey {
        let servers = servers
            .iter()
            .map(|(name, at, enabled)| {
                let transport = if at.starts_with("https:") {
                    McpTransport::Remote {
                        url: (*at).to_owned(),
                        headers: BTreeMap::new(),
                    }
                } else {
                    McpTransport::Local {
                        command: (*at).to_owned(),
                        args: Vec::new(),
                        env: BTreeMap::new(),
                    }
                };
                let server = McpServerLaunch {
                    transport,
                    enabled: *enabled,
                    scope: None,
                };
                ((*name).to_owned(), server)
            })
            .filter(|(_, server)| server.enabled)
            .map(|(name, server)| {
                let rev = revs.iter().find(|(n, _)| *n == name).map_or(0, |(_, r)| *r);
                (name, (server, rev))
            })
            .collect();
        ToolsKey {
            rev: 0,
            servers,
            ..ToolsKey::default()
        }
    }

    #[test]
    fn a_skill_or_a_builtin_switched_in_the_matter_changes_the_key() {
        let before = key(&[("notes", "npx", true)], &[]);
        let mut skill = before.clone();
        skill.skills.insert("pdf".to_owned(), "sha256:a".to_owned());
        assert!(!before.same_for(&skill, true));
        let mut browser = before.clone();
        browser.builtin_off.insert("browser");
        assert!(!before.same_for(&browser, false));
    }

    #[test]
    fn a_remote_server_counts_only_for_an_agent_that_takes_one() {
        let before = key(&[("notes", "npx", true)], &[]);
        let after = key(
            &[("notes", "npx", true), ("web", "https://a.test", true)],
            &[],
        );
        assert!(before.same_for(&after, false));
        assert!(!before.same_for(&after, true));
    }

    #[test]
    fn a_keychain_write_or_a_changed_entry_changes_the_key() {
        let before = key(&[("notes", "npx", true)], &[]);
        assert!(!before.same_for(&key(&[("notes", "npx", true)], &[("notes", 1)]), false));
        assert!(!before.same_for(&key(&[("notes", "uvx", true)], &[]), false));
        assert!(before.same_for(&key(&[("notes", "npx", true)], &[("other", 3)]), false));
        assert!(!before.same_for(&key(&[], &[]), false));
    }

    #[test]
    fn debug_shows_counts_only() {
        let key = key(&[("web", "https://secret.test/path", true)], &[]);
        let shown = format!("{key:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("servers: 1"), "{shown}");
    }
}
