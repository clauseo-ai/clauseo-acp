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
//! - **The user tier**: `HKCU\SOFTWARE\Policies\ClaudeCode` `Settings`, which only IT or a
//!   program running as administrator can write (the lawyer's account has read access only to
//!   `HKCU\Software\Policies`: laptop check, 16 Nov). Claude Code takes it as the policy tier
//!   when no admin tier has content (`iS`/`aS`). One that then weakens the launch refuses it,
//!   and stops a live conversation at its next prompt ([`user_policy_touches`]).
//!
//! Every tier's settings that weaken the launch are found by [`weakens::weakening`], named
//! with their document ([`Sourced`]) so the firm's IT can act; a value that only tightens
//! passes. A document that is not there, or is empty, says nothing. One that cannot be read or
//! parsed (or is over the size cap) refuses the launch, as a weakening one does: an admin
//! document's also counts as content (so the user tier does not apply), and turns nothing off
//! for [`AdminPolicy::hooks_off`].

use std::path::Path;

use serde_json::{Map, Value};

use super::os_policy::TierRead;
use crate::agent::loads::pinned::Pin;

pub(crate) mod codex_env;
pub(crate) mod weakens;
use weakens::Weakening;

/// The keys that, set to `true` in a policy, drop every hook but the policy's own.
pub(crate) const HOOKS_OFF_KEYS: [&str; 2] = ["disableAllHooks", "allowManagedHooksOnly"];

/// The user tier's place, as a refusal names it.
pub(crate) const USER_TIER: &str = "HKCU\\SOFTWARE\\Policies\\ClaudeCode";

/// A policy document and where it was read: `managed-settings.json`, `managed-settings.d`,
/// `macOS managed preferences`, `HKLM\…`, `remote-settings.json` or [`USER_TIER`].
pub(crate) type Sourced = (&'static str, Doc);

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
    /// Whether the server-managed settings have content ([`AdminPolicy::with_remote`]).
    pub(crate) remote: bool,
}

impl AdminPolicy {
    /// What `docs`, the admin tiers' documents, say.
    pub(crate) fn of(docs: &[Doc]) -> Self {
        Self {
            content: docs.iter().any(Doc::content),
            hooks_off: docs.iter().find_map(Doc::hooks_off),
            remote: false,
        }
    }

    /// With the server-managed settings' document `remote`: its key that turns hooks off
    /// counts, its content is noted apart, and the user tier's precedence is left alone.
    #[must_use]
    pub(crate) fn with_remote(mut self, remote: &Doc) -> Self {
        self.hooks_off = self.hooks_off.or_else(|| remote.hooks_off());
        self.remote = remote.content();
        self
    }
}

/// What a policy document finds wrong with a launch whose pins are `pins`
/// (`pinned::security_pins`), with Claude's sandbox or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finding {
    /// A setting that weakens the launch.
    Weakens(Weakening),
    /// The document cannot be read, parsed, or is over the size cap: whether it weakens the
    /// launch is unknown, so it refuses the launch too (fails closed).
    Unreadable(&'static str),
}

impl Finding {
    /// What a refusal names: the setting's dotted key, or the unreadable document.
    pub(crate) fn setting(&self) -> String {
        match self {
            Self::Weakens(found) => found.dotted(),
            Self::Unreadable(source) => (*source).to_owned(),
        }
    }

    /// The setting the app names ([`weakens::policy_setting`]); `None` for a document that
    /// cannot be read.
    pub(crate) fn policy_setting(&self) -> Option<clauseo_protocol::PolicySetting> {
        match self {
            Self::Weakens(found) => weakens::policy_setting(found.key),
            Self::Unreadable(_) => None,
        }
    }

    /// Where it was found.
    pub(crate) fn source(&self) -> &'static str {
        match self {
            Self::Weakens(found) => found.source,
            Self::Unreadable(source) => *source,
        }
    }
}

/// The first of `docs` that cannot be read or holds a setting weakening a launch with `pins`,
/// with Claude's sandbox or not (`sandboxed`); with `hooks_too`, a hooks key set to `true`
/// counts as weakening (the user tier's rule).
pub(crate) fn finding(
    docs: &[Sourced],
    pins: &[Pin],
    sandboxed: bool,
    hooks_too: bool,
) -> Option<Finding> {
    docs.iter().find_map(|(source, doc)| match doc {
        Doc::Absent => None,
        Doc::Unreadable => Some(Finding::Unreadable(*source)),
        Doc::Settings(settings) => {
            weakens::weakening(settings, *source, pins, sandboxed, hooks_too).map(Finding::Weakens)
        }
    })
}

/// What the user tier's document `user` finds wrong with the launch ([`finding`], the hooks
/// keys included), when it applies: when no admin tier has content.
pub(crate) fn user_policy_touches(
    admin: AdminPolicy,
    user: &Doc,
    pins: &[Pin],
    sandboxed: bool,
) -> Option<Finding> {
    if admin.content {
        return None;
    }
    finding(&[(USER_TIER, user.clone())], pins, sandboxed, true)
}

/// The admin tiers' documents on this machine ([`sourced_admin_docs`] without their sources).
#[cfg(test)]
pub(crate) fn admin_docs(managed: &Path, os: bool) -> Vec<Doc> {
    sourced_admin_docs(managed, os)
        .into_iter()
        .map(|(_, doc)| doc)
        .collect()
}

/// The admin tiers' documents on this machine, each with where it was read: the policy files in
/// `managed`, then, with `os`, the managed-preferences property lists and HKLM's value. On a
/// launch prep's thread, or the actor's at a prompt: a few small reads.
pub(crate) fn sourced_admin_docs(managed: &Path, os: bool) -> Vec<Sourced> {
    let mut docs: Vec<Sourced> = match super::managed_files(managed) {
        Ok(files) => files
            .iter()
            .map(|file| {
                let main = file.file_name() == Some(std::ffi::OsStr::new("managed-settings.json"));
                let source = if main {
                    "managed-settings.json"
                } else {
                    "managed-settings.d"
                };
                (source, file_doc(super::read(file)))
            })
            .collect(),
        Err(()) => vec![("managed-settings.d", Doc::Unreadable)],
    };
    if os {
        docs.extend(super::os_policy::plist_paths().iter().map(|path| {
            let doc = match super::read(path) {
                Ok(None) => Doc::Absent,
                Ok(Some(bytes)) => plist_doc(&bytes),
                Err(()) => Doc::Unreadable,
            };
            ("macOS managed preferences", doc)
        }));
        if let Some(hklm) = super::os_policy::registry_values().first() {
            docs.push(("HKLM\\SOFTWARE\\Policies\\ClaudeCode", Doc::from_read(hklm)));
        }
    }
    docs
}

/// The server-managed settings' document in Claude Code's config folder `config`.
pub(crate) fn remote_doc(config: &Path) -> Doc {
    file_doc(super::read(&config.join(REMOTE)))
}

/// The server-managed settings' file name, as a refusal names it.
pub(crate) const REMOTE: &str = "remote-settings.json";

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

/// A managed-preferences property list's document: its top-level dictionary as settings, each
/// value as JSON ([`plist_json`]), so a nested `sandbox` dictionary is read as an object.
pub(crate) fn plist_doc(bytes: &[u8]) -> Doc {
    if bytes.is_empty() {
        return Doc::Absent;
    }
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) else {
        return Doc::Unreadable;
    };
    match plist_json(&value) {
        Value::Object(settings) => Doc::Settings(settings),
        _ => Doc::Settings(Map::new()),
    }
}

/// A property list's value as JSON: dictionaries, arrays, strings, booleans and numbers as
/// themselves; dates, data and anything else as `null`.
fn plist_json(value: &plist::Value) -> Value {
    match value {
        plist::Value::Dictionary(items) => Value::Object(
            items
                .iter()
                .map(|(key, value)| (key.clone(), plist_json(value)))
                .collect(),
        ),
        plist::Value::Array(items) => Value::Array(items.iter().map(plist_json).collect()),
        plist::Value::Boolean(flag) => Value::Bool(*flag),
        plist::Value::String(text) => Value::from(text.as_str()),
        plist::Value::Integer(number) => number
            .as_signed()
            .map(Value::from)
            .or_else(|| number.as_unsigned().map(Value::from))
            .unwrap_or(Value::Null),
        plist::Value::Real(number) => Value::from(*number),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests;
