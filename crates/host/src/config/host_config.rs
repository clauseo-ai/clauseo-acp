//! `HostConfig` itself: what a host is started with; its builders are in `config.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_counts::Counts;
use clauseo_limits::ReadLimits;
use clauseo_process::Launcher;
use clauseo_settings::Effective;

use super::{IdleStop, LaunchWiring, ProfileHook, Sandbox, UpdateSeams};
use crate::clock::Clock;
use crate::ids::IdSource;
use crate::search::SearchSeams;

/// How to start a host. The data folder is `Effective::data_dir()`.
#[non_exhaustive]
pub struct HostConfig {
    pub(crate) effective: Effective,
    pub(super) launcher: Arc<dyn Launcher>,
    pub(super) profile: Box<dyn ProfileHook>,
    pub(super) refused_folders: Vec<PathBuf>,
    pub(super) documents: Option<crate::actor::documents::Later>,
    pub(super) home_folder: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(super) ids: Box<dyn IdSource>,
    pub(super) patch_window_bytes: usize,
    pub(super) managed_mcp: PathBuf,
    pub(super) app_capacity: usize,
    pub(super) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pub(super) folder_check: crate::agent::folder_support::CheckSource,
    pub(super) pre_write_guard: Option<PathBuf>,
    pub(super) limits_reader: Option<Arc<dyn ReadLimits>>,
    pub(crate) wiring: LaunchWiring,
    pub(super) scan_walk: crate::actor::scan::Walk,
    pub(super) scratch: crate::actor::scratch::Seams,
    pub(super) folder_probes: crate::actor::FolderProbes,
    pub(super) counts: Option<Counts>,
    pub(super) search: SearchSeams,
    pub(super) update: UpdateSeams,
    pub(super) idle: IdleStop,
    pub(super) sandbox: Option<Sandbox>,
}
