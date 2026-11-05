//! [`Effective`]: the settings `host` and `cli` run with. Read at start-up; the `ui` section
//! changes at runtime through [`Effective::apply_ui_patch`], the `tools` section through
//! [`Effective::apply_tools_patch`], the `privacy` section through
//! [`Effective::apply_privacy_patch`], `update.mode` through [`Effective::apply_update_mode`];
//! one agent's launch entry through [`Effective::set_agent_launch`], and one agent's plan
//! through [`Effective::set_plan`].

#[cfg(test)]
mod mcp_tests;
#[cfg(test)]
mod plans_tests;
#[cfg(test)]
mod privacy_tests;
#[cfg(test)]
mod set_tests;
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
use crate::leaf::{self, Leaf};
use crate::locations::Locations;
use crate::plans;
use crate::policy::{PolicyRead, PolicySource, PolicyStatus};
use crate::privacy::PrivacyPatch;
use crate::schema::{AgentLaunch, McpServerLaunch, Plan, Settings};
use crate::tools::ToolsPatch;
use crate::ui::UiPatch;
use crate::update::UpdateChoice;
use crate::user;
use crate::warning::{Origin, Warning, WarningKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What [`Effective::set_agent_launch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum EntryWrite {
    /// The entry was written to `settings.json` and is in force.
    Written,
    /// The entry there is the lawyer's or the firm's own (neither its program nor a `node`'s
    /// script is under `<data_dir>/agents/`), so it was left as it is and nothing was written.
    LeftAlone,
}

/// Why [`Effective::set_agent_launch`] refuses a launch that is not Clauseo's own install.
const NOT_OURS: &str = "Clauseo writes only its own installs: a command under \
                        <data_dir>/agents/, or node with its script there";

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
    /// `tools` and `privacy` key `false`, `update.mode` `none`, each `install` key unset, no
    /// plan at all), with a warning, since its value is unknown (2026-11-02, R21; 0036; R23;
    /// 0048). A plan a stronger source set stands. A policy `true` for a `privacy` key is
    /// ignored with a warning and locks nothing: a policy can only turn those off (0036). A
    /// policy may set `update.mode` to any value, and it locks.
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
                        lock_and_apply(leaf, &mut settings, &mut locked);
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

    /// Writes the launch entry `agents.<id>`: saves it to `settings.json` through `files`,
    /// then sets it in memory. Only Clauseo's own installs are written: `launch` must have an
    /// absolute `command` that lies under `<data_dir>/agents/`, or is a `node` (or `node.exe`)
    /// whose first argument (the adapter's script) lies there, or it is refused with
    /// [`Error::Refused`]. So is an `env` that sets a name holding `KEY`, `TOKEN`, `SECRET` or
    /// `PASSWORD` to anything but empty: keys live only in the keychain. The refusal names the
    /// key, never the value.
    ///
    /// A policy-locked id is refused with [`Error::Locked`]. An entry already there that is not
    /// Clauseo's by the same rule (or that does not type) is the lawyer's or the firm's own: it
    /// is never replaced, and the answer is
    /// [`EntryWrite::LeftAlone`] with nothing written. A failed write returns the error with
    /// nothing changed in memory.
    pub fn set_agent_launch(
        &mut self,
        files: &dyn Files,
        id: &str,
        launch: &AgentLaunch,
    ) -> Result<EntryWrite, Error> {
        let key = format!("agents.{id}");
        if self.is_locked(&key) {
            return Err(Error::Locked { key });
        }
        let data_dir = self.locations.data_dir();
        if id.is_empty()
            || !Path::new(&launch.command).is_absolute()
            || !plans::launch_is_clauseos(launch, data_dir)
        {
            return Err(Error::Refused {
                key,
                reason: NOT_OURS,
            });
        }
        plans::check_launch_env(id, launch)?;
        if self
            .settings
            .agents
            .get(id)
            .is_some_and(|held| !plans::launch_is_clauseos(held, data_dir))
        {
            return Ok(EntryWrite::LeftAlone);
        }
        let entry = serde_json_lenient::to_value(launch).map_err(|error| Error::Io {
            path: self.locations.settings_file().to_owned(),
            source: error.into(),
        })?;
        let keep = |held: &serde_json::Value| !plans::entry_is_clauseos(held, data_dir);
        if !user::write_entry(&self.locations, files, ("agents", id), Some(entry), keep)? {
            return Ok(EntryWrite::LeftAlone);
        }
        self.settings.agents.insert(id.to_owned(), launch.clone());
        Ok(EntryWrite::Written)
    }

    /// Writes the plan `plans.<id>`, or with `None` (or an empty plan) removes it: saves it to
    /// `settings.json` through `files`, then sets it in memory. A policy-locked id is refused
    /// with [`Error::Locked`]; while a policy source cannot be read every plan is locked. An
    /// id that is not an agent id, an endpoint that is not an ASCII `https` URL with a host (no
    /// user name, password, query, fragment or backslash, a port of at most 65535), a variable
    /// a plan may not set (anything but [`crate::PLAN_ENV_NAMES`] and [`crate::PLAN_PROXY_NAMES`])
    /// or a proxy address with a user name is refused with [`Error::Refused`], naming the key
    /// (or the variable's place, for a name that is no variable name) and never the value.
    /// Nothing is written then; a failed write returns the error with nothing changed in memory.
    pub fn set_plan(
        &mut self,
        files: &dyn Files,
        id: &str,
        plan: Option<&Plan>,
    ) -> Result<(), Error> {
        let key = format!("plans.{id}");
        if self.is_locked(&key) {
            return Err(Error::Locked { key });
        }
        if !plans::id_ok(id) {
            return Err(Error::Refused {
                key,
                reason: plans::BAD_ID,
            });
        }
        let plan = plan.filter(|plan| **plan != Plan::default());
        if let Some(plan) = plan {
            plans::check_plan(id, plan)?;
        }
        let entry = plan
            .map(serde_json_lenient::to_value)
            .transpose()
            .map_err(|error| Error::Io {
                path: self.locations.settings_file().to_owned(),
                source: error.into(),
            })?;
        let _ = user::write_entry(&self.locations, files, ("plans", id), entry, |_| false)?;
        match plan {
            Some(plan) => {
                self.settings.plans.insert(id.to_owned(), plan.clone());
            }
            None => {
                self.settings.plans.remove(id);
            }
        }
        Ok(())
    }

    /// How to launch the agent `id`, if one is set.
    pub fn agent(&self, id: &str) -> Option<&AgentLaunch> {
        self.settings.agents.get(id)
    }

    /// The agent `id`'s firm plan, if one is set (`plans.<id>`).
    pub fn plan(&self, id: &str) -> Option<&Plan> {
        self.settings.plans.get(id)
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

    /// Whether policy set the dotted `key` (such as `matters.default_location`, `mcp.servers`,
    /// `agents.<id>` or `plans.<id>`), so the user cannot change it. A locked key ending in `.`
    /// locks every key under it: `plans.`, while a policy source cannot be read.
    pub fn is_locked(&self, key: &str) -> bool {
        self.locked.contains(key) || under_section_lock(&self.locked, key)
    }

    /// Every locked key, sorted. One ending in `.` (`plans.`) stands for every key under it.
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
        if lock_and_apply(Leaf::Refused(key.to_owned()), settings, locked) {
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

/// Locks `leaf`'s key and applies it as policy, unless a stronger source locked that key, or a
/// section over it (`plans.`), already. Each plan a stronger source locked stands through a
/// refusal of the whole section. Returns whether it applied.
fn lock_and_apply(leaf: Leaf, settings: &mut Settings, locked: &mut BTreeSet<String>) -> bool {
    let key = leaf.key();
    if locked.contains(&key) || under_section_lock(locked, &key) {
        return false;
    }
    let kept: Vec<(String, Plan)> = settings
        .plans
        .iter()
        .filter(|(id, _)| locked.contains(&format!("plans.{id}")))
        .map(|(id, plan)| (id.clone(), plan.clone()))
        .collect();
    locked.insert(key);
    leaf.apply_policy(settings);
    settings.plans.extend(kept);
    true
}

/// Whether a locked section (a key ending in `.`) holds `key`, which is not that section.
fn under_section_lock(locked: &BTreeSet<String>, key: &str) -> bool {
    locked
        .iter()
        .any(|lock| lock.ends_with('.') && key.len() > lock.len() && key.starts_with(lock.as_str()))
}
