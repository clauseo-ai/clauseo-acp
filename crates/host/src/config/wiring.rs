//! What the launch wiring holds (`LaunchWiring`, the host's per-launch seams and hooks), and
//! its two hook types. Kept apart from `config.rs`, whose `HostConfig` builds it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::launch_gate::LaunchGate;
use crate::agent::login_link::SignIn;
use crate::browser::chrome::ChromeRoots;
use crate::browser::install::InstallTools;
use crate::browser::limits::BrowserSlots;

use super::EgressProxy;

/// A hook run with a session's run folder just before it is removed
/// (`HostConfig::with_run_removal_hook`).
pub(crate) type RunRemovalHook = Arc<dyn Fn(&std::path::Path) + Send + Sync>;

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
    /// Desktop's launch gate (`HostConfig::with_launch_gate`); `None`: no identity check is
    /// wired, and every launch of an agent with a kind is refused.
    pub(crate) launch_gate: Option<LaunchGate>,
    /// Run between a skill pack's two reads, for tests (`actor/settings/packs.rs`).
    pub(crate) pack_read_hook: Option<PackReadHook>,
    /// The case-law connector's relays (R24); `None`: no session gets case law.
    pub(crate) relays: Option<crate::connectors::Relays>,
    /// The connector commands' wiring (R24, `with_connectors`); `None`: each is refused.
    pub(crate) connectors: Option<crate::connectors::ConnectorsWiring>,
    /// Run off the actor with a run folder's path just before the folder is removed, for tests
    /// (`launch_checks::discard`).
    pub(crate) run_removal_hook: Option<RunRemovalHook>,
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
            launch_gate: None,
            pack_read_hook: None,
            relays: None,
            connectors: None,
            run_removal_hook: None,
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
            .field("launch_gate", &self.launch_gate.is_some())
            .field("relays", &self.relays)
            .field("connectors", &self.connectors)
            .finish_non_exhaustive()
    }
}
