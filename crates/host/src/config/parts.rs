//! What `HostConfig::into_parts` hands the host thread (`ConfigParts`) and the updater's seams
//! (`UpdateSeams`). Kept apart from `config.rs`, whose `HostConfig` builds them.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_counts::Counts;
use clauseo_limits::ReadLimits;
use clauseo_process::Launcher;
use clauseo_settings::Effective;

use crate::clock::Clock;
use crate::ids::IdSource;
use crate::search::SearchSeams;
use crate::update::Updater;

use super::{IdleStop, LaunchWiring, ProfileHook};

/// A `HostConfig` taken apart by the host thread.
pub(crate) struct ConfigParts {
    pub(crate) effective: Effective,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) profile: Box<dyn ProfileHook>,
    pub(crate) refused_folders: Vec<PathBuf>,
    pub(crate) home_folder: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Box<dyn IdSource>,
    pub(crate) patch_window_bytes: usize,
    pub(crate) managed_mcp: PathBuf,
    pub(crate) app_capacity: usize,
    pub(crate) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pub(crate) folder_check: crate::agent::folder_support::CheckSource,
    pub(crate) pre_write_guard: Option<PathBuf>,
    /// The limits reader a caller set; `None` builds the default at start-up.
    pub(crate) limits_reader: Option<Arc<dyn ReadLimits>>,
    pub(crate) wiring: LaunchWiring,
    pub(crate) scan_walk: crate::actor::scan::Walk,
    pub(crate) scratch: crate::actor::scratch::Seams,
    pub(crate) folder_probes: crate::actor::FolderProbes,
    /// The usage counts a caller set; `None` counts nothing.
    pub(crate) counts: Option<Counts>,
    /// The search thread's test seams.
    pub(crate) search: SearchSeams,
    /// The updater and what its machine starts from.
    pub(crate) update: UpdateSeams,
    /// The idle stop's limit and live-agents cap.
    pub(crate) idle: IdleStop,
}

/// The updater and what its machine starts from (decision record 0037).
pub(crate) struct UpdateSeams {
    pub(crate) updater: Arc<dyn Updater>,
    /// The seed of the check delays; `None` seeds from the clock at start.
    pub(crate) jitter_seed: Option<u64>,
    /// The app's version, which `UpdateView::current` shows and the marker compares.
    pub(crate) app_version: String,
    /// The store's close reports a failure once it has closed (`with_failing_close`).
    #[cfg(feature = "test-support")]
    pub(crate) fail_close: bool,
}
