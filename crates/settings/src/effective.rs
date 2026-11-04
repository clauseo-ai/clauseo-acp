//! [`Effective`]: the settings `host` and `cli` run with. Read at start-up; the `ui` section
//! changes at runtime through [`Effective::apply_ui_patch`], the `tools` section through
//! [`Effective::apply_tools_patch`], the `privacy` section through
//! [`Effective::apply_privacy_patch`], `update.mode` through [`Effective::apply_update_mode`].

#[cfg(test)]
mod mcp_tests;
#[cfg(test)]
mod privacy_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tools_patch_tests;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod ui_patch_tests;
#[cfg(test)]
mod update_tests;

use crate::error::Error;
use crate::files::Files;
use crate::leaf;
use crate::locations::Locations;
use crate::policy::{PolicyRead, PolicySource, PolicyStatus};
use crate::privacy::PrivacyPatch;
use crate::schema::{AgentLaunch, McpServerLaunch, Settings};
use crate::tools::ToolsPatch;
use crate::ui::UiPatch;
use crate::update::UpdateChoice;
use crate::user;
use crate::warning::{Origin, Warning, WarningKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The settings after the defaults, the user's `settings.json` and the policy layer, in that
/// order: policy wins. The `ui` and `tools` sections change at runtime only through
/// [`Effective::apply_ui_patch`] and [`Effective::apply_tools_patch`]; everything else takes
/// effect on the next start. A session reads its `tools` at its start, so a change to them
/// reaches the next session, not one already running.
#[derive(Debug)]
pub struct Effective {
    locations: Locations,
    settings: Settings,
    policy: PolicyStatus,
    policy_present: bool,
    locked: BTreeSet<String>,
    warnings: Vec<Warning>,
}

impl Effective {
    /// Reads `settings.json` and each policy source, and merges them. Never fails: a missing
    /// file is normal, and anything that cannot be read becomes a warning or a source error,
    /// with defaults in its place.
    ///
    /// `policy` lists the sources strongest first: a key the first source sets is not taken
    /// from a later one. A policy value replaces the user's value for its key, and an agent's
    /// replaces the user's whole entry for that id; each such key is locked. A policy value for a
    /// known key that does not type locks the key at its default (an agent: no entry), or off for
    /// a security key (`leaf::SECURITY_KEYS`), with a warning: fail closed (Rohan, 2026-10-31). A
    /// source that cannot be read also locks each security key off (`mcp.servers` empty, each
    /// `tools` and `privacy` key `false`, `update.mode` `none`), with a warning, since its value
    /// is unknown (2026-11-02, R21; 0036; R23). A policy `true` for a `privacy` key is ignored
    /// with a warning and locks nothing: a policy can only turn those off (0036). A policy may
    /// set `update.mode` to any value, and it locks.
    pub fn load(
        locations: &Locations,
        files: &dyn Files,
        policy: &[&dyn PolicySource],
    ) -> Effective {
        let mut warnings = Vec::new();
        let mut settings = Settings::default();
        for leaf in user::read(locations.settings_file(), files, &mut warnings) {
            leaf.apply(&mut settings);
        }
        let mut status = PolicyStatus::default();
        let mut locked = BTreeSet::new();
        let mut policy_present = false;
        for source in policy {
            match source.read() {
                PolicyRead::Absent => {}
                PolicyRead::Unreadable(error) => {
                    policy_present = true;
                    status.push_error(source.name(), error);
                    lock_security_keys(source.name(), &mut settings, &mut locked, &mut warnings);
                }
                PolicyRead::Read(values) => {
                    policy_present = true;
                    if values.sets_a_value() {
                        status.push_applied(source.name());
                    }
                    let (leaves, source_warnings) = values.into_parts();
                    warnings.extend(source_warnings);
                    for leaf in leaves {
                        if leaf.turns_on_an_off_only_key() {
                            warnings.push(policy_on_ignored(source.name(), leaf.key()));
                            continue;
                        }
                        if locked.insert(leaf.key()) {
                            leaf.apply_policy(&mut settings);
                        }
                    }
                }
            }
        }
        Effective {
            locations: locations.clone(),
            settings,
            policy: status,
            policy_present,
            locked,
            warnings,
        }
    }

    /// Every setting.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the `ui` section: saves the patch to `settings.json` through `files`, then sets
    /// it in memory. It is the one runtime write; a caller reads the result from
    /// [`Effective::settings`].
    ///
    /// A patch that names any key policy locked is refused as a whole with
    /// [`Error::Locked`], the first locked key in field order, even when the patch also sets
    /// unlocked fields or the value equals the locked one: nothing is written and nothing
    /// changes. An empty patch returns `Ok` without touching the file, so its comments stay. A
    /// failed write (an [`Error::Io`], or an [`Error::Parse`] of a file that does not parse)
    /// returns the error with nothing changed in memory.
    pub fn apply_ui_patch(&mut self, files: &dyn Files, patch: &UiPatch) -> Result<(), Error> {
        let keys = patch.keys();
        if let Some(key) = keys.iter().find(|key| self.is_locked(key)) {
            return Err(Error::Locked {
                key: (*key).to_owned(),
            });
        }
        if keys.is_empty() {
            return Ok(());
        }
        user::write_user_settings(&self.locations, files, patch)?;
        self.settings.apply_ui_patch(patch);
        Ok(())
    }

    /// Changes the `tools` section: saves the patch to `settings.json` through `files`, then
    /// sets it in memory, as [`Effective::apply_ui_patch`] does for `ui`. A patch that names
    /// `tools.web_search` or `tools.browser` while policy locks it is refused as a whole with
    /// [`Error::Locked`], the first locked key in field order; nothing is written or changed.
    /// An empty patch returns `Ok` without touching the file. A failed write returns the error
    /// with nothing changed in memory. A session reads these switches at its start, so the
    /// change reaches the next session started, not one running.
    pub fn apply_tools_patch(
        &mut self,
        files: &dyn Files,
        patch: &ToolsPatch,
    ) -> Result<(), Error> {
        let keys = patch.keys();
        if let Some(key) = keys.iter().find(|key| self.is_locked(key)) {
            return Err(Error::Locked {
                key: (*key).to_owned(),
            });
        }
        if keys.is_empty() {
            return Ok(());
        }
        user::write_tools_settings(&self.locations, files, patch)?;
        self.settings.tools.apply_patch(patch);
        Ok(())
    }

    /// Whether the `privacy` patch could be applied as far as policy goes: [`Error::Locked`],
    /// the first locked key in field order, when it names a key policy locks. The host calls it
    /// before it turns counts on, so a refused patch changes nothing anywhere;
    /// [`Effective::apply_privacy_patch`] checks again.
    pub fn check_privacy_patch(&self, patch: &PrivacyPatch) -> Result<(), Error> {
        match patch.keys().into_iter().find(|key| self.is_locked(key)) {
            Some(key) => Err(Error::Locked {
                key: key.to_owned(),
            }),
            None => Ok(()),
        }
    }

    /// Changes the `privacy` section: saves the patch to `settings.json` through `files`, then
    /// sets it in memory, as [`Effective::apply_ui_patch`] does for `ui`. A patch that names
    /// `privacy.counts` or `privacy.crash_reports` while policy locks it is refused as a whole
    /// with [`Error::Locked`] ([`Effective::check_privacy_patch`]); nothing is written or
    /// changed. An empty patch returns `Ok` without touching the file. A failed write returns
    /// the error with nothing changed in memory. The counts themselves are not here: the host
    /// turns `clauseo-counts` on or off around this call (decision record 0036).
    pub fn apply_privacy_patch(
        &mut self,
        files: &dyn Files,
        patch: &PrivacyPatch,
    ) -> Result<(), Error> {
        self.check_privacy_patch(patch)?;
        if patch.keys().is_empty() {
            return Ok(());
        }
        user::write_privacy_settings(&self.locations, files, patch)?;
        self.settings.privacy.apply_patch(patch);
        Ok(())
    }

    /// Changes `update.mode` to the lawyer's choice: saves it to `settings.json` through `files`,
    /// then sets it in memory, as [`Effective::apply_privacy_patch`] does for `privacy`. While
    /// policy locks the key (a policy value, or a policy source that cannot be read) it is
    /// refused with [`Error::Locked`]; nothing is written or changed. A failed write returns the
    /// error with nothing changed in memory. `none` cannot be written: [`UpdateChoice`] has no
    /// such value, since only the firm's policy (or a hand edit) turns updates off. The host
    /// calls it for `SetUpdateMode`.
    pub fn apply_update_mode(
        &mut self,
        files: &dyn Files,
        mode: UpdateChoice,
    ) -> Result<(), Error> {
        const KEY: &str = "update.mode";
        if self.is_locked(KEY) {
            return Err(Error::Locked {
                key: KEY.to_owned(),
            });
        }
        user::write_update_mode(&self.locations, files, mode)?;
        self.settings.update.mode = mode.into();
        Ok(())
    }

    /// How to launch the agent `id`, if one is set.
    pub fn agent(&self, id: &str) -> Option<&AgentLaunch> {
        self.settings.agents.get(id)
    }

    /// Every MCP server in the settings, on or off, by name. When policy sets `mcp.servers`
    /// this is the firm's list alone.
    pub fn mcp_servers(&self) -> &BTreeMap<String, McpServerLaunch> {
        &self.settings.mcp.servers
    }

    /// The MCP servers turned on, in name order: the only ones an agent's session gets.
    pub fn enabled_mcp_servers(&self) -> impl Iterator<Item = (&str, &McpServerLaunch)> {
        self.settings
            .mcp
            .servers
            .iter()
            .filter(|(_, server)| server.enabled)
            .map(|(name, server)| (name.as_str(), server))
    }

    /// Where new matters go: `matters.default_location` (user or policy), or else
    /// [`Locations::default_matters_dir`]: `Documents/Clauseo`, or with `CLAUSEO_DATA_DIR` set the
    /// sibling `<data dir>-matters`.
    pub fn default_matters_dir(&self) -> &Path {
        self.settings
            .matters
            .default_location
            .as_deref()
            .unwrap_or_else(|| self.locations.default_matters_dir())
    }

    /// The data folder.
    pub fn data_dir(&self) -> &Path {
        self.locations.data_dir()
    }

    /// The folders it was loaded from.
    pub fn locations(&self) -> &Locations {
        &self.locations
    }

    /// Which policy sources applied, and which could not be read.
    pub fn policy(&self) -> &PolicyStatus {
        &self.policy
    }

    /// Whether any policy source was there at start-up: one that was read, even one silent on
    /// every key this version knows, or one that could not be read. `false` only when every
    /// source was absent. It is the usage tally's `managed` (decision record 0036); unlike
    /// [`PolicyStatus::is_absent`] it counts a source that set no valid value. Beside
    /// [`Effective::is_locked`]: a present policy need not lock anything.
    pub fn policy_present(&self) -> bool {
        self.policy_present
    }

    /// Whether policy set the dotted `key` (such as `matters.default_location`, `mcp.servers`
    /// or `agents.<id>`), so the user cannot change it.
    pub fn is_locked(&self, key: &str) -> bool {
        self.locked.contains(key)
    }

    /// Every locked key, sorted.
    pub fn locked(&self) -> impl Iterator<Item = &str> {
        self.locked.iter().map(String::as_str)
    }

    /// What loading worked round, in the order it was found.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// The warning for a policy `true` on a key a policy may only turn off
/// (`leaf::OFF_ONLY_KEYS`): it is ignored and locks nothing.
fn policy_on_ignored(name: &str, key: String) -> Warning {
    Warning {
        origin: Origin::Store {
            name: name.to_owned(),
        },
        kind: WarningKind::InvalidValue {
            message: format!(
                "a policy can only turn {key} off, so true is ignored and the lawyer's switch stands"
            ),
            key,
        },
    }
}

/// Locks each security key a stronger source has not set off ([`leaf::refuse`]), for the
/// unreadable source `name`: what it held for the key is unknown, so the user's value must not
/// stand in for it, and the default may be on.
fn lock_security_keys(
    name: &str,
    settings: &mut Settings,
    locked: &mut BTreeSet<String>,
    warnings: &mut Vec<Warning>,
) {
    for key in leaf::SECURITY_KEYS {
        if locked.insert(key.to_owned()) {
            leaf::refuse(key, settings);
            warnings.push(Warning {
                origin: Origin::Store {
                    name: name.to_owned(),
                },
                kind: WarningKind::InvalidValue {
                    key: key.to_owned(),
                    message: "the policy source could not be read, so this is locked off"
                        .to_owned(),
                },
            });
        }
    }
}
