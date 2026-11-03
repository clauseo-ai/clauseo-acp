//! The launch prep's browser part: which sessions get one, the sidecar check, the Chrome search
//! and the session's claims (`browser::session`). See `launch_checks.rs` and decision record
//! 0029.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::super::Actor;
use crate::browser::chrome::ChromeRoots;
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;

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
                let file = data
                    .join(crate::browser::CONFIG_DIR)
                    .join(crate::browser::CONFIG_FILE);
                tracing::warn!(file = %file.display(), %error, "agent-browser's config file was not written; the session has no browser");
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
    /// Where the socket folder goes; `None` is `/tmp`.
    #[cfg(unix)]
    socket_root: Option<PathBuf>,
}

impl BrowserPrep {
    /// The Chrome found and the session's claims. A missing sidecar leaves both out, with a
    /// warning (a development build has no agent-browser); a failed claim leaves the claims
    /// out. Either way the entry drops with `BrowserNotInstalled`. File system and socket
    /// calls: off the actor.
    pub(super) fn prepare(
        self,
        data: &Path,
        session_id: &str,
    ) -> (Option<PathBuf>, Option<Arc<BrowserSession>>) {
        if matches!(self.chrome, ChromeSearch::Skip) {
            return (None, None);
        }
        if let Some(missing) = self.sidecars.iter().find(|sidecar| !sidecar.is_file()) {
            tracing::warn!(sidecar = %missing.display(), "a browser sidecar is missing; the session has no browser");
            return (None, None);
        }
        let Some(chrome) = self.chrome.find_with_config(data) else {
            return (None, None);
        };
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
                (Some(chrome), Some(Arc::new(session)))
            }
            Err(error) => {
                tracing::warn!(%error, "the session has no browser");
                (Some(chrome), None)
            }
        }
    }
}

/// What the launch prep does for `agent`'s session `key`, from the actor's state: nothing when
/// no browser can go to it (`tools.browser` off, no proxy, no slot, or no `browser` entry from
/// the profile hook).
pub(super) fn browser_prep(
    actor: &Actor,
    agent: &clauseo_protocol::AgentId,
    key: SessionKey,
) -> BrowserPrep {
    let skip = BrowserPrep {
        sidecars: Vec::new(),
        chrome: ChromeSearch::Skip,
        #[cfg(unix)]
        socket_root: None,
    };
    if !actor.effective.settings().tools.browser
        || actor.wiring.proxy.is_none()
        || !actor.wiring.browser_slots.free_for(key)
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
    }
}
