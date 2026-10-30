//! [`Effective`]: the settings `host` and `cli` run with, read once at start-up.

#[cfg(test)]
mod tests;

use crate::files::Files;
use crate::locations::Locations;
use crate::policy::{PolicyRead, PolicySource, PolicyStatus};
use crate::schema::{AgentLaunch, Settings};
use crate::user;
use crate::warning::Warning;
use std::collections::BTreeSet;
use std::path::Path;

/// The settings after the defaults, the user's `settings.json` and the policy layer, in that
/// order: policy wins. Read once and never changed; a change takes effect on the next start.
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
    /// replaces the user's whole entry for that id; each such key is locked.
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
                    status.push_applied(source.name());
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

    /// The folders it was loaded from, for [`crate::write_user_settings`], so a caller that
    /// holds the `Effective` needs no second `Locations`.
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
