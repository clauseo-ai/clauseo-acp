//! What Claude Code's policy tier says about Clauseo's guard hook and the launch's own rules
//! (audit row 13 of b4l-sbxrefuse; rows 19 and 21). Policy settings outrank Clauseo's flag
//! settings (user, then flag, then policy; Claude Code 2.1.293 `BSr`), so a policy key wins
//! whatever the launch sends:
//!
//! - **Admin tiers**: `managed-settings.json` and its drop-ins (as [`super::managed_files`]),
//!   the macOS managed-preferences property lists and the registry's
//!   `HKLM\SOFTWARE\Policies\ClaudeCode` `Settings` value ([`super::os_policy`]). Only an
//!   administrator can write them. One setting `disableAllHooks` or `allowManagedHooksOnly` to
//!   `true` turns Clauseo's guard hook off (`if(t.disableAllHooks===!0)return o?.hooks??{}`;
//!   decision record 0030): [`AdminPolicy::hooks_off`] names the key. Every admin document is
//!   read, though Claude Code may skip one a higher one outranks: a key in one it skips only
//!   costs a refusal it did not need.
//! - **Server-managed settings**: `<config>/remote-settings.json`, Claude Code's cache of the
//!   settings the firm sets for its Claude organisation, in Clauseo's own Claude home (the
//!   launch's `CLAUDE_CONFIG_DIR`; Claude Code writes it there when the account has them).
//!   Read as an admin tier for [`AdminPolicy::hooks_off`] ([`AdminPolicy::with_remote`]), so a
//!   firm's server policy turning hooks off is handled as a file's is. It does not count as
//!   [`AdminPolicy::content`]: the file sits in a folder the user can write, so a planted one
//!   must not switch the user tier's check off.
//! - **The user tier**: `HKCU\SOFTWARE\Policies\ClaudeCode` `Settings`, which any program the
//!   user runs can write. Claude Code takes it as the policy tier when no admin tier has content
//!   (`iS`/`aS`). One that then sets anything under [`GUARDED_KEYS`] refuses Claude's launch,
//!   and stops a live conversation at its next prompt ([`user_policy_touches`]).
//!
//! A document that is not there, or is empty, says nothing. An admin document that cannot be
//! read or parsed counts as content (so the user tier does not apply) but turns nothing off
//! here; the user tier's value that cannot be read or parsed counts as touching the keys.

use std::path::Path;

use serde_json::{Map, Value};

use super::os_policy::TierRead;

/// The keys that, set to `true` in a policy, drop every hook but the policy's own.
pub(crate) const HOOKS_OFF_KEYS: [&str; 2] = ["disableAllHooks", "allowManagedHooksOnly"];

/// The top-level keys of a user-writable policy that refuse Claude's launch: the permissions,
/// sandbox and hooks areas.
pub(crate) const GUARDED_KEYS: [&str; 6] = [
    "permissions",
    "sandbox",
    "hooks",
    "disableAllHooks",
    "allowManagedHooksOnly",
    "allowManagedPermissionRulesOnly",
];

/// One policy document as read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Doc {
    /// Not there, or empty.
    Absent,
    /// There, but it cannot be read or parsed.
    Unreadable,
    /// Its settings object (a document that is no object reads as an empty one).
    Settings(Map<String, Value>),
}

impl Doc {
    /// A JSON document's text, as a file or a registry value holds it.
    pub(crate) fn from_text(text: &str) -> Self {
        let text = text.trim().trim_start_matches('\u{feff}').trim();
        if text.is_empty() {
            return Self::Absent;
        }
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(settings)) => Self::Settings(settings),
            Ok(_) => Self::Settings(Map::new()),
            Err(_) => Self::Unreadable,
        }
    }

    /// A tier read's document.
    pub(crate) fn from_read(read: &TierRead) -> Self {
        match read {
            Ok(None) => Self::Absent,
            Ok(Some(text)) => Self::from_text(text),
            Err(()) => Self::Unreadable,
        }
    }

    /// Whether it holds any setting, or cannot be read.
    fn content(&self) -> bool {
        match self {
            Self::Absent => false,
            Self::Unreadable => true,
            Self::Settings(settings) => !settings.is_empty(),
        }
    }

    /// The first of [`HOOKS_OFF_KEYS`] it sets to `true`.
    fn hooks_off(&self) -> Option<&'static str> {
        let Self::Settings(settings) = self else {
            return None;
        };
        HOOKS_OFF_KEYS
            .into_iter()
            .find(|key| settings.get(*key) == Some(&Value::Bool(true)))
    }
}

/// What the admin tiers say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AdminPolicy {
    /// Whether an admin tier has content, so the user tier does not apply.
    pub(crate) content: bool,
    /// The key that turns Clauseo's guard hook off, when one does.
    pub(crate) hooks_off: Option<&'static str>,
    /// Whether an admin document could not be read or parsed.
    pub(crate) unreadable: bool,
    /// Whether the server-managed settings have content ([`AdminPolicy::with_remote`]).
    pub(crate) remote: bool,
}

impl AdminPolicy {
    /// What `docs`, the admin tiers' documents, say.
    pub(crate) fn of(docs: &[Doc]) -> Self {
        Self {
            content: docs.iter().any(Doc::content),
            hooks_off: docs.iter().find_map(Doc::hooks_off),
            unreadable: docs.contains(&Doc::Unreadable),
            remote: false,
        }
    }

    /// With the server-managed settings' document `remote`: its key that turns hooks off
    /// counts, its content is noted apart, and the user tier's precedence is left alone.
    #[must_use]
    pub(crate) fn with_remote(mut self, remote: &Doc) -> Self {
        self.hooks_off = self.hooks_off.or_else(|| remote.hooks_off());
        self.remote = remote.content();
        self.unreadable |= *remote == Doc::Unreadable;
        self
    }
}

/// Whether the user tier's document `user` applies (no admin tier has content) and sets a key
/// of [`GUARDED_KEYS`], or cannot be read.
pub(crate) fn user_policy_touches(admin: AdminPolicy, user: &Doc) -> bool {
    if admin.content {
        return false;
    }
    match user {
        Doc::Absent => false,
        Doc::Unreadable => true,
        Doc::Settings(settings) => GUARDED_KEYS.iter().any(|key| settings.contains_key(*key)),
    }
}

/// The admin tiers' documents on this machine: the policy files in `managed`, then, with `os`,
/// the managed-preferences property lists and HKLM's value. On a launch prep's thread, or the
/// actor's at a prompt: a few small reads.
pub(crate) fn admin_docs(managed: &Path, os: bool) -> Vec<Doc> {
    let mut docs: Vec<Doc> = match super::managed_files(managed) {
        Ok(files) => files
            .iter()
            .map(|file| file_doc(super::read(file)))
            .collect(),
        Err(()) => vec![Doc::Unreadable],
    };
    if os {
        docs.extend(
            super::os_policy::plist_paths()
                .iter()
                .map(|path| match super::read(path) {
                    Ok(None) => Doc::Absent,
                    Ok(Some(bytes)) => plist_doc(&bytes),
                    Err(()) => Doc::Unreadable,
                }),
        );
        if let Some(hklm) = super::os_policy::registry_values().first() {
            docs.push(Doc::from_read(hklm));
        }
    }
    docs
}

/// The server-managed settings' document in Claude Code's config folder `config`.
pub(crate) fn remote_doc(config: &Path) -> Doc {
    file_doc(super::read(&config.join("remote-settings.json")))
}

/// The user tier's value on this machine (HKCU's); not there but on Windows. Unit tests play
/// their own (`profile_loads/policy.rs`).
#[cfg(not(test))]
pub(crate) fn user_tier() -> TierRead {
    super::os_policy::registry_values()
        .get(1)
        .cloned()
        .unwrap_or(Ok(None))
}

/// A policy file's document.
fn file_doc(read: Result<Option<Vec<u8>>, ()>) -> Doc {
    match read {
        Ok(None) => Doc::Absent,
        Ok(Some(bytes)) => std::str::from_utf8(&bytes).map_or(Doc::Unreadable, Doc::from_text),
        Err(()) => Doc::Unreadable,
    }
}

/// A managed-preferences property list's document: its top-level dictionary, whose keys are
/// the settings' (only their presence and `true` booleans matter here).
pub(crate) fn plist_doc(bytes: &[u8]) -> Doc {
    if bytes.is_empty() {
        return Doc::Absent;
    }
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) else {
        return Doc::Unreadable;
    };
    let Some(dictionary) = value.as_dictionary() else {
        return Doc::Settings(Map::new());
    };
    let settings = dictionary
        .iter()
        .map(|(key, value)| {
            let value = value.as_boolean().map_or(Value::Null, Value::Bool);
            (key.clone(), value)
        })
        .collect();
    Doc::Settings(settings)
}

#[cfg(test)]
mod tests;
