//! The launch prep's browser part: which sessions get one, the firm's browser policy
//! (`browser::rd_policy`), the sidecar check, the Chrome search with a downloaded Chrome for
//! Testing judged by the signed browser pins (`browser::chrome::find_checked`,
//! `browser::pins`) and the session's claims (`browser::session`). See `launch_checks.rs` and
//! decision record 0029.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::super::Actor;
use crate::browser::chrome::{ChromeRoots, Picked};
use crate::browser::pins::{self, PinsSource};
use crate::browser::rd_policy::RdPolicy;
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use clauseo_protocol::{Timestamp, ToolNotice, ToolsUnavailableReason};

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
    /// The Chrome found, a downloaded Chrome for Testing judged by the newest verified pins in
    /// `pins` at `now` (read only then); a forced one is never judged. File system calls: off
    /// the actor.
    fn find(
        self,
        data: &Path,
        pins: &PinsSource,
        now: Timestamp,
    ) -> Result<Picked, ToolsUnavailableReason> {
        match self {
            Self::Skip => Err(ToolsUnavailableReason::BrowserNotInstalled),
            Self::Forced(chrome) => Ok(Picked {
                path: chrome,
                pins_published: None,
            }),
            Self::Roots(roots) => {
                let roots = roots.unwrap_or_else(|| ChromeRoots::system(data));
                crate::browser::chrome::find_checked(&roots, now, || pins.newest(now))
            }
        }
    }

    /// The Chrome found, once agent-browser's config file is in place
    /// (`browser::write_config`). Without the file the guard cannot start, so a failure to
    /// write it leaves the browser out (`BrowserNotInstalled`), with a warning; the launch goes
    /// on. File system calls: off the actor.
    fn find_with_config(
        self,
        data: &Path,
        pins: &PinsSource,
        now: Timestamp,
    ) -> Result<Picked, ToolsUnavailableReason> {
        let chrome = self.find(data, pins, now)?;
        match crate::browser::write_config(data) {
            Ok(_) => Ok(chrome),
            Err(error) => {
                tracing::warn!(
                    error = %crate::log_file::Cause(&error),
                    "agent-browser's config file was not written; the session has no browser"
                );
                Err(ToolsUnavailableReason::BrowserNotInstalled)
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
    /// Where the signed browser pins are read from, when a downloaded Chrome for Testing is
    /// found.
    pins: PinsSource,
    /// The host clock's time when the launch prep was made: pins dated over a day ahead of it
    /// are set aside, and its notice is judged by it.
    now: Timestamp,
    /// Where the socket folder goes; `None` is `/tmp`.
    #[cfg(unix)]
    socket_root: Option<PathBuf>,
}

impl BrowserPrep {
    /// Whether the launch prep looks for the session's browser at all: `tools.browser` on, a
    /// proxy set, a slot free and a `browser` entry from the hook. The session's egress proxy
    /// opens only then.
    pub(super) fn wanted(&self) -> bool {
        !matches!(self.chrome, ChromeSearch::Skip)
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
    /// `BrowserBlockedByPolicy` or `BrowserPolicyUnreadable`. A downloaded Chrome for Testing
    /// the pins refuse leaves both out too, with `BrowserOutOfDate`, `BrowserListUnavailable` or
    /// `BrowserListExpired`; one they take carries [`ToolNotice::BrowserListOld`] when they are
    /// over 14 days old. A missing sidecar leaves both out, with a warning (a development build has
    /// no agent-browser); a failed claim leaves the claims out. Either way the entry drops with
    /// `BrowserNotInstalled`. Registry, file system and socket calls: off the actor.
    pub(super) fn prepare(self, data: &Path, session_id: &str) -> Prepared {
        if matches!(self.chrome, ChromeSearch::Skip) {
            return Prepared::default();
        }
        if let Some(reason) = self.rd_policy.check(self.allowed_anyway) {
            return Prepared {
                blocked: Some(reason),
                ..Prepared::default()
            };
        }
        if self.sidecars.iter().any(|sidecar| !sidecar.is_file()) {
            tracing::warn!("a browser sidecar is missing; the session has no browser");
            return Prepared::default();
        }
        let picked = match self.chrome.find_with_config(data, &self.pins, self.now) {
            Ok(picked) => picked,
            Err(ToolsUnavailableReason::BrowserNotInstalled) => return Prepared::default(),
            Err(reason) => {
                return Prepared {
                    blocked: Some(reason),
                    ..Prepared::default()
                };
            }
        };
        let notice = picked
            .pins_published
            .and_then(|published| pins::notice(published, self.now));
        let chrome = picked.path;
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
                    blocked: None,
                    notice,
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %crate::log_file::Cause(&error),
                    "the session has no browser"
                );
                Prepared {
                    chrome: Some(chrome),
                    ..Prepared::default()
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
    /// Why the browser was left out before any claim, if it was: the firm's browser policy
    /// (`browser::rd_policy`), or the pins refusing a downloaded Chrome for Testing.
    pub(super) blocked: Option<ToolsUnavailableReason>,
    /// The notice the browser's header carries, if any (`browser::pins::notice`).
    pub(super) notice: Option<ToolNotice>,
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

/// What the launch prep does for `agent`'s session `key`, from the actor's state: nothing when
/// no browser can go to it (`tools.browser` off, no proxy, no `browser` entry from the profile
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
        pins: actor.wiring.browser_pins.source(actor.effective.data_dir()),
        now: actor.clock.now().wall,
        #[cfg(unix)]
        socket_root: None,
    };
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
    let chrome = match &actor.wiring.browser_executable {
        Some(chrome) => ChromeSearch::Forced(chrome.clone()),
        None => ChromeSearch::Roots(actor.wiring.chrome_roots.clone()),
    };
    BrowserPrep {
        sidecars,
        chrome,
        #[cfg(unix)]
        socket_root: actor.wiring.browser_socket_root.clone(),
        ..skip
    }
}
