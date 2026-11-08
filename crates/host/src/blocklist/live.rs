//! The settings the updater reads each round, shared with the actor, which puts the effective
//! settings in on every settings change (`actor/app.rs`'s `publish` of `SettingsChanged`). The
//! policy file is read only at start, so a policy change reaches it at the next start.

use std::sync::{Arc, PoisonError, RwLock};

use clauseo_settings::Settings;

/// What a round needs of the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fetching {
    /// `network.unsafe_site_list_updates`: false, no round fetches.
    pub(crate) updates: bool,
    /// `network.upstream_proxy`: the route of every request when set.
    pub(crate) upstream_proxy: Option<String>,
}

impl Fetching {
    /// The block list's part of `settings`.
    pub(crate) fn of(settings: &Settings) -> Self {
        Self {
            updates: settings.network.unsafe_site_list_updates,
            upstream_proxy: settings.network.upstream_proxy.clone(),
        }
    }
}

/// [`Fetching`], shared: the actor writes it, the updater reads it at each round's start.
#[derive(Debug, Clone)]
pub(crate) struct Live(Arc<RwLock<Fetching>>);

impl Live {
    /// Starts at `fetching`.
    pub(crate) fn new(fetching: Fetching) -> Self {
        Self(Arc::new(RwLock::new(fetching)))
    }

    /// The settings now.
    pub(crate) fn get(&self) -> Fetching {
        // A plain value: whatever a panicking writer left is still whole.
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Replaces them.
    pub(crate) fn set(&self, fetching: Fetching) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = fetching;
    }
}
