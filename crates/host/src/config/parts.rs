//! What `HostConfig` taken apart (`ConfigParts::from`) hands the host thread (`ConfigParts`) and
//! the updater's seams (`UpdateSeams`), and two test seams of `HostConfig` that `config.rs` has
//! no room for. Kept apart from `config.rs`, whose `HostConfig` builds them.

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

use super::{HostConfig, IdleStop, LaunchWiring, ProfileHook, Sandbox};

/// A `HostConfig` taken apart by the host thread.
pub(crate) struct ConfigParts {
    pub(crate) effective: Effective,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) profile: Box<dyn ProfileHook>,
    pub(crate) refused_folders: Vec<PathBuf>,
    /// The default matters folder's late answer (`with_documents_later`), if it comes late.
    pub(crate) documents: Option<crate::actor::documents::Later>,
    pub(crate) home_folder: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Box<dyn IdSource>,
    pub(crate) patch_window_bytes: usize,
    pub(crate) managed_mcp: PathBuf,
    pub(crate) app_capacity: usize,
    pub(crate) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pub(crate) folder_check: crate::agent::folder_support::CheckSource,
    pub(crate) pre_write_guard: Option<PathBuf>,
    pub(crate) guard_selftest: bool,
    pub(crate) settings_canary: bool,
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
    /// The sandbox seam (`with_sandbox`), if any.
    pub(crate) sandbox: Option<Sandbox>,
    /// The last run did not quit through its own quit (`with_last_run_unclean`).
    pub(crate) last_run_unclean: bool,
}

/// Tests: the cloud-only probe of every matter walk (`clauseo_matters::Settings::probe`;
/// default the OS's `OsProbe`), so a test can pretend some files are placeholders. A trait
/// beside `HostConfig`'s own methods, as `config.rs` has no room for one more.
#[cfg(feature = "test-support")]
pub trait WithPlaceholderProbe {
    /// This config, its matter walks asking `probe` whether a file is cloud-only.
    #[must_use]
    fn with_placeholder_probe(
        self,
        probe: Arc<dyn clauseo_matters::PlaceholderProbe + Send + Sync>,
    ) -> Self;
}

#[cfg(feature = "test-support")]
impl WithPlaceholderProbe for HostConfig {
    fn with_placeholder_probe(
        mut self,
        probe: Arc<dyn clauseo_matters::PlaceholderProbe + Send + Sync>,
    ) -> Self {
        self.folder_probes.placeholder = probe;
        self
    }
}

/// Tests: where the matter workers' undo and restore deletes go
/// (`clauseo_matters::Settings::trash`; default the OS's trash), so a test run never fills the
/// machine's Trash or Recycle Bin. A trait beside `HostConfig`'s own methods, as
/// [`WithPlaceholderProbe`] is.
#[cfg(feature = "test-support")]
pub trait WithMatterTrash {
    /// This config, its matter workers moving each file they delete into the folder `folder`
    /// (`clauseo_matters::FolderTrash`).
    #[must_use]
    fn with_matter_trash(self, folder: PathBuf) -> Self;
}

#[cfg(feature = "test-support")]
impl WithMatterTrash for HostConfig {
    fn with_matter_trash(mut self, folder: PathBuf) -> Self {
        self.folder_probes.trash = Some(Arc::new(clauseo_matters::FolderTrash::new(folder)));
        self
    }
}

/// Takes the config apart.
impl From<HostConfig> for ConfigParts {
    fn from(config: HostConfig) -> Self {
        Self {
            effective: config.effective,
            launcher: config.launcher,
            profile: config.profile,
            refused_folders: config.refused_folders,
            documents: config.documents,
            home_folder: config.home_folder,
            clock: config.clock,
            ids: config.ids,
            patch_window_bytes: config.patch_window_bytes,
            app_capacity: config.app_capacity,
            managed_mcp: config.managed_mcp,
            codex_hooks: config.codex_hooks,
            folder_check: config.folder_check,
            pre_write_guard: config.pre_write_guard,
            guard_selftest: config.guard_selftest,
            settings_canary: config.settings_canary,
            limits_reader: config.limits_reader,
            wiring: config.wiring,
            scan_walk: config.scan_walk,
            scratch: config.scratch,
            folder_probes: config.folder_probes,
            counts: config.counts,
            search: config.search,
            // Before `update` moves.
            last_run_unclean: config.update.last_run_unclean,
            update: config.update,
            idle: config.idle,
            sandbox: config.sandbox,
        }
    }
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
    /// The last run did not quit through its own quit (`HostConfig::with_last_run_unclean`).
    pub(crate) last_run_unclean: bool,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "config.rs is at its 600-line limit; the last run's flag lives beside its part"
)]
impl HostConfig {
    /// The program Claude Code's `PreToolUse` hook runs, with the one argument
    /// [`pre_write_guard::FLAG`](crate::pre_write_guard::FLAG), before each file edit and shell
    /// command (default: none, and no hook goes). The desktop passes its own executable, whose
    /// `main` runs [`pre_write_guard::run`](crate::pre_write_guard::run) on that argument.
    /// `agent/loads.rs` builds the hook entry.
    pub fn with_pre_write_guard(mut self, program: PathBuf) -> Self {
        self.pre_write_guard = Some(program);
        self
    }

    /// No guard program, as when the desktop cannot name its own executable: no hook goes, and
    /// a Claude launch that rests on the guard is refused (`agent.guard_missing`).
    pub fn without_pre_write_guard(mut self) -> Self {
        self.pre_write_guard = None;
        self
    }

    /// Whether each Claude launch first runs the guard's hook command, exactly as its entry
    /// states it, on a made-up tool call it must refuse, and is refused
    /// (`agent.guard_selftest_failed`) when it does not refuse in time (`guard_selftest.rs`,
    /// `pre_write_guard/selftest.rs`). Default: on, but off in tests and in a build with
    /// `test-support`, whose tests name a guard program that is not there.
    #[must_use]
    pub fn with_guard_selftest(mut self, on: bool) -> Self {
        self.guard_selftest = on;
        self
    }

    /// Whether each Claude session setup (`session/new`, `session/load`, `session/resume`) is
    /// checked for the nonce its `SessionStart` canary writes, proof that Claude Code loaded
    /// the flag settings, and refused (`agent.settings_not_loaded`) without it. Default: on,
    /// but off in tests and in a build with `test-support`, whose fake agent runs no hook.
    #[must_use]
    pub fn with_settings_canary(mut self, on: bool) -> Self {
        self.settings_canary = on;
        self
    }

    /// The run before this one did not quit through its own quit (default: it did): the desktop's
    /// mark in the data folder was still there. `LastExit` says so, for the room's notice, with
    /// the file actions the store says that run left unfinished.
    #[must_use]
    pub fn with_last_run_unclean(mut self, unclean: bool) -> Self {
        self.update.last_run_unclean = unclean;
        self
    }
}
