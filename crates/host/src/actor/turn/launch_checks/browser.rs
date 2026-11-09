//! The launch prep's browser part: which sessions get one, the firm's browser policy
//! (`browser::rd_policy`), the sidecar check, the Chrome search and the session's claims
//! (`browser::session`). See `launch_checks.rs` and decision record
//! 0029.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::super::Actor;
use crate::browser::chrome::ChromeRoots;
use crate::browser::rd_policy::RdPolicy;
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use clauseo_protocol::ToolsUnavailableReason;

/// How the launch prep picks the session's Chrome.
enum ChromeSearch {
    /// No browser can go to this session (`tools.browser` off, or no proxy): no search.
    Skip,
    /// The Chrome the tests forced.
    Forced(PathBuf),
    /// Search these roots; `None` is this machine's, with the data folder as app data.
    Roots(Option<ChromeRoots>),
}

impl ChromeSearch {
    /// The Chrome found. File system calls: off the actor.
    fn find(self, data: &Path) -> Option<PathBuf> {
        match self {
            Self::Skip => None,
            Self::Forced(chrome) => Some(chrome),
            Self::Roots(roots) => {
                let roots = roots.unwrap_or_else(|| ChromeRoots::system(data));
                crate::browser::chrome::find(&roots)
            }
        }
    }

    /// The Chrome found, once agent-browser's config file is in place
    /// (`browser::write_config`). Without the file the guard cannot start, so a failure to
    /// write it leaves the browser out (`BrowserNotInstalled`), with a warning; the launch goes
    /// on. File system calls: off the actor.
    fn find_with_config(self, data: &Path) -> Option<PathBuf> {
        let chrome = self.find(data)?;
        match crate::browser::write_config(data) {
            Ok(_) => Some(chrome),
            Err(error) => {
                tracing::warn!(
                    error = %crate::log_file::Cause(&error),
                    "agent-browser's config file was not written; the session has no browser"
                );
                None
            }
        }
    }
}

/// What the launch prep does for the session's browser: the sidecars to check, the Chrome
/// search, and where the socket folder goes.
pub(super) struct BrowserPrep {
    /// The browser entry's `clauseo-mcp` (its command) and agent-browser (its
    /// `--agent-browser` argument), as the profile hook gave them.
    sidecars: Vec<PathBuf>,
    chrome: ChromeSearch,
    /// Reads the firm's Chrome, Edge and Chromium `RemoteDebuggingAllowed` (the wiring's).
    rd_policy: Arc<RdPolicy>,
    /// The firm's `browser.allow_when_remote_debugging_disabled`.
    allowed_anyway: bool,
    /// Where the socket folder goes; `None` is `/tmp`.
    #[cfg(unix)]
    socket_root: Option<PathBuf>,
    /// The browser as a shell tool (`HostConfig::with_browser_tool`): no MCP entry, no proxy,
    /// no slot and no claims; the Chrome found is the session's `AGENT_BROWSER_EXECUTABLE_PATH`.
    shell: bool,
}

impl BrowserPrep {
    /// Whether the launch prep looks for the session's browser at all: `tools.browser` on, a
    /// proxy set, a slot free and a `browser` entry from the hook; or, for the shell tool,
    /// `tools.browser` on. The session's egress proxy opens only for the entry, once claimed.
    pub(super) fn wanted(&self) -> bool {
        !matches!(self.chrome, ChromeSearch::Skip)
    }

    /// Whether this is the shell tool's prep (`HostConfig::with_browser_tool`).
    pub(super) fn shell(&self) -> bool {
        self.shell
    }

    /// The prep with no browser: a launch whose slot wait ended with every slot held
    /// ([`slot_wait`]) looks for no Chrome and claims nothing, so it holds no browser port and
    /// opens no proxy.
    pub(super) fn skipped(mut self) -> Self {
        self.sidecars = Vec::new();
        self.chrome = ChromeSearch::Skip;
        self
    }

    /// The Chrome found and the session's claims. The firm's browser policy comes first,
    /// before any Chrome is picked, forced or found (`browser::rd_policy`), read again at each
    /// launch: when it blocks, both are left out and the entry drops with
    /// `BrowserBlockedByPolicy` or `BrowserPolicyUnreadable`. A missing sidecar
    /// leaves both out, with a warning (a development build has no agent-browser); a failed
    /// claim leaves the claims out. Either way the entry drops with `BrowserNotInstalled`.
    /// Registry, file system and socket calls: off the actor.
    pub(super) fn prepare(self, data: &Path, session_id: &str) -> Prepared {
        if matches!(self.chrome, ChromeSearch::Skip) {
            return Prepared::default();
        }
        let policy_block = self.rd_policy.check(self.allowed_anyway);
        let read = Prepared {
            policy_read: true,
            policy_block,
            ..Prepared::default()
        };
        if policy_block.is_some() {
            return read;
        }
        if self.sidecars.iter().any(|sidecar| !sidecar.is_file()) {
            tracing::warn!("a browser sidecar is missing; the session has no browser");
            return read;
        }
        let Some(chrome) = self.chrome.find_with_config(data) else {
            return read;
        };
        if self.shell {
            // agent-browser starts its own Chrome: nothing to claim.
            return Prepared {
                chrome: Some(chrome),
                ..read
            };
        }
        let claimed = BrowserSession::claim(
            session_id,
            #[cfg(unix)]
            self.socket_root.as_deref(),
        );
        match claimed {
            Ok(session) => {
                tracing::debug!(
                    %session_id,
                    ports = ?session.ports.loopback(),
                    "claimed the browser's loopback ports"
                );
                Prepared {
                    chrome: Some(chrome),
                    session: Some(Arc::new(session)),
                    ..read
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %crate::log_file::Cause(&error),
                    "the session has no browser"
                );
                Prepared {
                    chrome: Some(chrome),
                    ..read
                }
            }
        }
    }
}

/// What [`BrowserPrep::prepare`] found for the session.
#[derive(Debug, Default)]
pub(super) struct Prepared {
    /// The Chrome picked, if any.
    pub(super) chrome: Option<PathBuf>,
    /// The session's claims, if made.
    pub(super) session: Option<Arc<BrowserSession>>,
    /// Why the firm's browser policy left the browser out, if it did (`browser::rd_policy`).
    pub(super) policy_block: Option<ToolsUnavailableReason>,
    /// Whether the firm's browser policy was read for this launch (`actor/browser_policy.rs`).
    pub(super) policy_read: bool,
}

/// How long a launch prep waits for a browser slot that a running removal may free
/// ([`slot_wait`]). A removal whose processes are gone ends within a few looks; one that waits
/// for a straggler can run to its cap (20 s), and the launch goes on without it.
pub(super) const SLOT_WAIT: Duration = Duration::from_secs(3);

/// The wait of a launch prep that wants the browser (`prep`) for `key` while every slot is
/// taken and a holder's removal runs: until a holder's removal ends or [`SLOT_WAIT`] has
/// passed on the host clock. It answers whether a slot is free then; without one the prep
/// claims nothing for the browser and opens no proxy (`BrowserPrep::skipped`). The session
/// looks at the slots again (`profile_loads.rs`), so it gets the slot that removal freed.
///
/// No removal can end unseen between the look at the slots and the wait: both happen here, in
/// one actor step (`launch_checks::prepare`, called on the actor), and a removal's end is
/// `removal::on_run_removed`, also on the actor, which wakes every registered wait. A removal
/// that ends after this step finds the wait registered, and its wake makes the wait ready at
/// once, before or after the launch prep first polls it.
pub(super) fn slot_wait(
    actor: &mut Actor,
    key: SessionKey,
    prep: &BrowserPrep,
) -> Option<impl std::future::Future<Output = bool> + Send + use<>> {
    if matches!(prep.chrome, ChromeSearch::Skip) || !actor.wiring.browser_slots.releasing(key) {
        return None;
    }
    let (wake, woken) = futures::channel::oneshot::channel();
    actor.turn_path.slot_waits.push(wake);
    let timer = actor.clock.after(SLOT_WAIT);
    tracing::debug!("a launch waits for a browser slot that a session's end frees");
    Some(async move {
        match futures::future::select(woken, timer).await {
            futures::future::Either::Left((Ok(free), _)) => free,
            // The host is going away, or the wait ran out with every slot still held.
            futures::future::Either::Left((Err(_), _)) | futures::future::Either::Right(_) => false,
        }
    })
}

/// Whether a slot is there for the browser after a wait (without one, it is); then the block
/// list's first load, before the proxy opens (`blocklist/first_load.rs`).
pub(super) async fn slot_then_block_list(
    slot_wait: Option<impl std::future::Future<Output = bool>>,
    block_list_wait: Option<impl std::future::Future<Output = ()>>,
) -> bool {
    let slot = match slot_wait {
        Some(slot_wait) => slot_wait.await,
        None => true,
    };
    if let Some(block_list_wait) = block_list_wait {
        block_list_wait.await;
    }
    slot
}

/// Warns, once per launch (`on_prepared`), when `run`'s prep waited for a slot
/// ([`slot_wait`]) and every slot is still held: the launch goes on without the browser
/// (`TooManyBrowsers`). A launch that found no slot and nothing releasing does not wait and
/// gives no warning.
pub(super) fn warn_if_still_full(actor: &Actor, run: &super::RunFolders) {
    if run.slot_waited && !actor.wiring.browser_slots.free_for(run.key) {
        tracing::warn!(
            matter = %run.key.matter,
            thread = %run.key.thread,
            launch = run.key.launch,
            "a launch went on without the browser: every slot still held after waiting for an \
             ended session's cleanup"
        );
    }
}

/// What the launch prep does for `agent`'s session `key`, from the actor's state. With the shell
/// tool wired (`HostConfig::with_browser_tool`): its `agent-browser` command and a Chrome search
/// when `tools.browser` is on, else nothing. Otherwise, for the guarded entry: nothing when no
/// browser can go to it (`tools.browser` off, no proxy, no `browser` entry from the profile
/// hook, or no slot and none coming free).
pub(super) fn browser_prep(
    actor: &Actor,
    agent: &clauseo_protocol::AgentId,
    key: SessionKey,
) -> BrowserPrep {
    let settings = actor.effective.settings();
    let skip = BrowserPrep {
        sidecars: Vec::new(),
        chrome: ChromeSearch::Skip,
        rd_policy: Arc::clone(&actor.wiring.rd_policy),
        allowed_anyway: settings.browser.allow_when_remote_debugging_disabled,
        #[cfg(unix)]
        socket_root: None,
        shell: false,
    };
    if let Some(tools) = &actor.wiring.browser_tool {
        if !settings.tools.browser {
            return skip;
        }
        let command = tools.join(format!(
            "{}{}",
            clauseo_profile::paths::AGENT_BROWSER_COMMAND,
            std::env::consts::EXE_SUFFIX
        ));
        return BrowserPrep {
            sidecars: vec![command],
            chrome: chrome_search(actor),
            shell: true,
            ..skip
        };
    }
    if !settings.tools.browser
        || actor.wiring.proxy.is_none()
        || !(actor.wiring.browser_slots.free_for(key) || actor.wiring.browser_slots.releasing(key))
    {
        return skip;
    }
    let entry = actor.profile.mcp_servers(agent).into_iter().find(|server| {
        let clauseo_acp::McpServer::Stdio { name, .. } = server;
        name == clauseo_profile::servers::BROWSER
    });
    let Some(clauseo_acp::McpServer::Stdio { command, args, .. }) = entry else {
        return skip;
    };
    let mut sidecars = vec![command];
    sidecars.extend(
        args.iter()
            .position(|arg| arg == "--agent-browser")
            .and_then(|at| args.get(at + 1))
            .map(PathBuf::from),
    );
    BrowserPrep {
        sidecars,
        chrome: chrome_search(actor),
        #[cfg(unix)]
        socket_root: actor.wiring.browser_socket_root.clone(),
        ..skip
    }
}

/// How the session's Chrome is picked: the one the tests forced, else a search of the wiring's
/// roots (this machine's when none).
fn chrome_search(actor: &Actor) -> ChromeSearch {
    match &actor.wiring.browser_executable {
        Some(chrome) => ChromeSearch::Forced(chrome.clone()),
        None => ChromeSearch::Roots(actor.wiring.chrome_roots.clone()),
    }
}
