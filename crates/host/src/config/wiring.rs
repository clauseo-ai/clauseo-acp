//! What the launch wiring holds (`LaunchWiring`, the host's per-launch seams and hooks), and
//! its two hook types. Kept apart from `config.rs`, whose `HostConfig` builds it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::actor::turn::HeldRuns;
use crate::agent::login_link::SignIn;
use crate::browser::chrome::ChromeRoots;
use crate::browser::install::InstallTools;
use crate::browser::limits::BrowserSlots;

use super::EgressProxy;

/// A hook run with a session's run folder just before it is removed
/// (`HostConfig::with_run_removal_hook`).
pub(crate) type RunRemovalHook = Arc<dyn Fn(&std::path::Path) + Send + Sync>;

/// A hook run as a session's removal starts waiting for its processes, with the session's
/// claimed Chrome debugging port (`HostConfig::with_tree_wait_hook`).
pub(crate) type TreeWaitHook = Arc<dyn Fn(Option<u16>) + Send + Sync>;

/// A hook run between a skill pack's two reads, with its id (`HostConfig::with_pack_read_hook`).
pub(crate) type PackReadHook = Arc<dyn Fn(&str) + Send + Sync>;

/// What the browser entry and the agents' `PATH` need beyond the hook: the proxy, stetdoc's
/// folder, where to look for Chrome, and the browser slots.
#[derive(Clone)]
pub(crate) struct LaunchWiring {
    pub(crate) proxy: Option<Arc<dyn EgressProxy>>,
    pub(crate) path_prepend: Option<PathBuf>,
    /// A Chrome the tests force, in place of the search.
    pub(crate) browser_executable: Option<PathBuf>,
    /// Where the launch prep looks for Chrome; `None` is this machine's roots
    /// (`ChromeRoots::system` with the data folder as app data).
    pub(crate) chrome_roots: Option<ChromeRoots>,
    /// The browser slots: the limit from the machine's memory, and the sessions holding one.
    pub(crate) browser_slots: BrowserSlots,
    /// macOS and Linux: where the browser socket folders go and the start-up sweep looks;
    /// `None` is `/tmp`. Tests pass a folder of their own.
    #[cfg(unix)]
    pub(crate) browser_socket_root: Option<PathBuf>,
    /// The environment an agent's spec starts from, in place of the host's own
    /// (`std::env::vars_os` at launch), for tests; `None` reads the host's.
    pub(crate) inherited_env: Option<Vec<(OsString, OsString)>>,
    /// The Chrome for Testing download's tools; `None` is `SystemTools` (curl, ditto, tar).
    pub(crate) install_tools: Option<Arc<dyn InstallTools>>,
    /// The sign-in hook, the sign-in-again test, and the host's sign-in state.
    pub(crate) sign_in: SignIn,
    /// Run between a skill pack's two reads, for tests (`actor/settings/packs.rs`).
    pub(crate) pack_read_hook: Option<PackReadHook>,
    /// The case-law connector's relays (R24); `None`: no session gets case law.
    pub(crate) relays: Option<crate::connectors::Relays>,
    /// The connector commands' wiring (R24, `with_connectors`); `None`: each is refused.
    pub(crate) connectors: Option<crate::connectors::ConnectorsWiring>,
    /// Run off the actor with a run folder's path just before the folder is removed, for tests
    /// (`launch_checks::discard`).
    pub(crate) run_removal_hook: Option<RunRemovalHook>,
    /// How long a run folder's removal waits for the agent's tree and its Chrome to be gone
    /// (`HostConfig::with_tree_wait`, default `DEFAULT_TREE_WAIT`).
    pub(crate) tree_wait: Duration,
    /// Run off the actor as a session's removal starts its wait, with the session's claimed
    /// Chrome debugging port, for tests (`HostConfig::with_tree_wait_hook`).
    pub(crate) tree_wait_hook: Option<TreeWaitHook>,
    /// Whether a run folder removal's thread fails to start, for tests
    /// (`HostConfig::with_removal_thread_failing`).
    pub(crate) removal_thread_fails: bool,
    /// The sessions whose removal ran past [`Self::tree_wait`]: their proxy, relay and claims,
    /// held until the host stops (`launch_checks::straggler`).
    pub(crate) held: HeldRuns,
    /// The block list's seams (ADR 0053, `BlockListSeams::apply`); `None`: no fetch.
    pub(crate) blocklist: Option<crate::blocklist::BlockListSeams>,
    /// The native dialogs (R18, `HostConfig::with_dialogs`); `None`: nothing can be widened.
    pub(crate) dialogs: Option<Arc<dyn crate::dialogs::NativeDialogs>>,
}

impl Default for LaunchWiring {
    fn default() -> Self {
        Self {
            proxy: None,
            path_prepend: None,
            browser_executable: None,
            chrome_roots: None,
            browser_slots: BrowserSlots::new(None),
            #[cfg(unix)]
            browser_socket_root: None,
            inherited_env: None,
            install_tools: None,
            sign_in: SignIn::default(),
            pack_read_hook: None,
            relays: None,
            connectors: None,
            run_removal_hook: None,
            tree_wait: super::DEFAULT_TREE_WAIT,
            tree_wait_hook: None,
            removal_thread_fails: false,
            held: HeldRuns::default(),
            blocklist: None,
            dialogs: None,
        }
    }
}

impl std::fmt::Debug for LaunchWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchWiring")
            .field("proxy", &self.proxy.is_some())
            .field("path_prepend", &self.path_prepend)
            .field("browser_executable", &self.browser_executable)
            .field("chrome_roots", &self.chrome_roots)
            .field("browser_slots", &self.browser_slots)
            // Names and values stay out: the environment holds secrets.
            .field("inherited_env", &self.inherited_env.is_some())
            .field("install_tools", &self.install_tools.is_some())
            .field("sign_in", &self.sign_in)
            .field("relays", &self.relays)
            .field("connectors", &self.connectors)
            .field("tree_wait", &self.tree_wait)
            .field("dialogs", &self.dialogs.is_some())
            .finish_non_exhaustive()
    }
}
