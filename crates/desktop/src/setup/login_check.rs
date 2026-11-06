//! The host's [`LoginCheck`] hook, from S18's checks (decision record 0048, ruling 2): desktop
//! answers it with `codex_link_holds` and `codex_login_shared`, so no file-id code goes into
//! `host` (ruling 27). Every answer is by stat only, never an open, and fails closed:
//!
//! - [`LoginCheck::check`] while desktop has told the host the login is reused: S18's link check,
//!   `Holds` as [`LoginLink::Shared`], `Split` as [`LoginLink::Split`] (a stat that failed on
//!   the home's file or a symbolic link included) and `Gone` as [`LoginLink::Gone`]. With no
//!   home for the lawyer it is `Split`: the launch is refused and the leave order runs.
//! - [`LoginCheck::check`] otherwise: [`LoginLink::Own`] only when S18 says the home's file is
//!   not the lawyer's; a link, a failed stat or no home for the lawyer is
//!   [`LoginLink::Shared`], so the API-key `authenticate` retry is not armed.
//! - [`LoginCheck::shared`]: `Yes` or `No` from `codex_login_shared`, and `Unknown` when a stat
//!   of either file fails for any reason but "not found", or the lawyer has no home. The host
//!   counts `Unknown` as shared.
//!
//! Its log lines name the agent and the answer, never a path.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_agents::{CodexLink, codex_link_holds, codex_login_shared};
use clauseo_host::{LoginCheck, LoginLink, Shared};
use clauseo_protocol::AgentId;

use crate::bridge::lock;

/// The agents whose login desktop last told the host is reused (`set_reused_login`), shared by
/// the setup backend, which writes it beside each call, and [`DesktopLoginCheck`].
#[derive(Debug, Default, Clone)]
pub(crate) struct ReusedLogins(Arc<Mutex<BTreeSet<AgentId>>>);

impl ReusedLogins {
    /// Records whether `agent`'s login is reused.
    pub(crate) fn set(&self, agent: &AgentId, reused: bool) {
        let mut agents = lock(&self.0);
        if reused {
            agents.insert(agent.clone());
        } else {
            agents.remove(agent);
        }
    }

    /// Whether `agent`'s login is reused, as desktop last said.
    pub(crate) fn get(&self, agent: &AgentId) -> bool {
        lock(&self.0).contains(agent)
    }
}

/// The hook: the lawyer's home (whose `.codex/auth.json` is the lawyer's login) and the
/// reused logins.
#[derive(Debug, Clone)]
pub(crate) struct DesktopLoginCheck {
    lawyer_home: Option<PathBuf>,
    reused: ReusedLogins,
}

impl DesktopLoginCheck {
    /// Checks against `lawyer_home` (`None` when this computer gives none: every answer then
    /// fails closed), reading the source from `reused`.
    pub(crate) fn new(lawyer_home: Option<PathBuf>, reused: ReusedLogins) -> Self {
        Self {
            lawyer_home,
            reused,
        }
    }
}

impl LoginCheck for DesktopLoginCheck {
    fn check(&self, agent: &AgentId, agent_home: &Path) -> LoginLink {
        let reused = self.reused.get(agent);
        let link = match (&self.lawyer_home, reused) {
            (None, true) => LoginLink::Split,
            (None, false) => LoginLink::Shared,
            (Some(lawyer), true) => match codex_link_holds(agent_home, lawyer) {
                CodexLink::Holds => LoginLink::Shared,
                CodexLink::Gone => LoginLink::Gone,
                // `Split`, and any newer answer S18 may add, which is no proof the link holds:
                // fail closed.
                _ => LoginLink::Split,
            },
            (Some(lawyer), false) => {
                if codex_login_shared(agent_home, lawyer) {
                    LoginLink::Shared
                } else {
                    LoginLink::Own
                }
            }
        };
        tracing::debug!(%agent, reused, ?link, "login check");
        link
    }

    fn shared(&self, agent: &AgentId, agent_home: &Path) -> Shared {
        let Some(lawyer) = &self.lawyer_home else {
            return Shared::Unknown;
        };
        let files = [
            agent_home.join(LOGIN_FILE),
            lawyer.join(LAWYER_CODEX).join(LOGIN_FILE),
        ];
        if files.iter().any(|file| stat_failed(file)) {
            tracing::debug!(%agent, "login check: a stat failed");
            return Shared::Unknown;
        }
        if codex_login_shared(agent_home, lawyer) {
            Shared::Yes
        } else {
            Shared::No
        }
    }
}

/// Codex's login file, in its home and in the lawyer's `.codex` (S18's `reuse.rs`).
pub(crate) const LOGIN_FILE: &str = "auth.json";
/// The lawyer's Codex folder under their home.
pub(crate) const LAWYER_CODEX: &str = ".codex";

/// Whether a stat of `file` (never following a link) failed for any reason but "not found".
fn stat_failed(file: &Path) -> bool {
    matches!(std::fs::symlink_metadata(file), Err(err) if err.kind() != io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests;
