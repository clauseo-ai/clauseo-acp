//! [`Effective`]: the settings `host` and `cli` run with. Read at start-up; the `ui` section
//! changes at runtime through [`Effective::apply_ui_patch`].

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_patch_tests;

use crate::error::Error;
use crate::files::Files;
use crate::locations::Locations;
use crate::policy::{PolicyRead, PolicySource, PolicyStatus};
use crate::schema::{AgentLaunch, Settings};
use crate::ui::UiPatch;
use crate::user;
use crate::warning::Warning;
use std::collections::BTreeSet;
use std::path::Path;

/// The settings after the defaults, the user's `settings.json` and the policy layer, in that
/// order: policy wins. The `ui` section changes at runtime only through
/// [`Effective::apply_ui_patch`]; everything else takes effect on the next start.
#[derive(Debug)]
pub struct Effective {
    locations: Locations,
    settings: Settings,
    policy: PolicyStatus,
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
    /// known key that does not type locks the key at its default (an agent: no entry), with a
    /// warning: fail closed (Rohan, 2026-10-31).
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
        for source in policy {
            match source.read() {
                PolicyRead::Absent => {}
                PolicyRead::Unreadable(error) => status.push_error(source.name(), error),
                PolicyRead::Read(values) => {
                    if values.sets_a_value() {
                        status.push_applied(source.name());
                    }
                    let (leaves, source_warnings) = values.into_parts();
                    warnings.extend(source_warnings);
                    for leaf in leaves {
                        if locked.insert(leaf.key()) {
                            leaf.apply(&mut settings);
                        }
                    }
                }
            }
        }
        Effective {
            locations: locations.clone(),
            settings,
            policy: status,
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

    /// How to launch the agent `id`, if one is set.
    pub fn agent(&self, id: &str) -> Option<&AgentLaunch> {
        self.settings.agents.get(id)
    }

    /// Where new matters go: `matters.default_location`, or else `Documents/Clauseo`.
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

    /// Whether policy set the dotted `key` (such as `matters.default_location` or
    /// `agents.<id>`), so the user cannot change it.
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
