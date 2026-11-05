//! The launch prep's case-law part (R24, decisions D3 and D4): the session's relay, opened on
//! the launch-prep thread beside its browser claims, and its token file.
//!
//! A session gets a relay only when every one of these holds; otherwise nothing is bound and no
//! file is written ([`CaseLaw`] says what the header shows instead):
//!
//! 1. The app has relays (`HostConfig::with_case_law_relays`) and the profile hook offers a
//!    `case-law` entry for the agent.
//! 2. The connector is added (`NotAdded`: nothing shows at all).
//! 3. `tools.connectors` is on (`Off`, or `LockedOff` when policy set it).
//! 4. No firm `managed-mcp.json` (`ManagedByFirm`: no Clauseo server goes at all).
//! 5. The connector is signed in (signed out or a revoked key: nothing shows in the header; the
//!    connector's own state says why).
//! 6. The token file's path is UTF-8, a loopback port binds (`sandbox::ports::bind_loopback`:
//!    the Windows range with `SO_EXCLUSIVEADDRUSE`, retired ports on Unix), the relay opens and
//!    the token file is written (`NoRelay` when one fails).
//!
//! The token goes to `<run>/relay-token` through `write::replace_private_file` (0600 on Unix,
//! in the 0700 run folder; on Windows it inherits the run folder's ACL, decision D8), and to the
//! agent's stderr redactor; never into the entry, an argument, a variable or a log. When the
//! session ends, [`end`] revokes the token (the relay answers `closed` from then on) before the
//! run folder, and the file with it, is removed; the relay itself stops when the last
//! [`RunFolders`] clone drops in `discard`, after the agent's tree was killed.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_connectors::{CredentialStatus, Secret};
use clauseo_protocol::{AgentId, ThreadId, ToolsUnavailableReason};

use super::super::super::Actor;
use super::super::state;
use super::RunFolders;
use crate::agent::loads::McpControl;
use crate::agent::loads::browser::Switch;
use crate::connectors::{RelaySession, Relays, TurnHandle};

/// The token file's name in the session's run folder.
pub(crate) const TOKEN_FILE: &str = "relay-token";

/// What the launch prep made of the session's case law.
#[derive(Debug, Clone, Default)]
pub(crate) enum CaseLaw {
    /// Nothing about case law shows: no relays, no `case-law` entry from the hook, or the
    /// connector not added, signed out or with its key revoked.
    #[default]
    Absent,
    /// The connector is added, and this session does not get it, for this reason.
    Unavailable(ToolsUnavailableReason),
    /// The relay listens and the token file is written.
    Open(Arc<CaseLawRelay>),
    /// The relay opened, then was ended before the launch, for this reason (the firm's
    /// `managed-mcp.json` appeared): no entry. It is kept so it stops off the actor, with the
    /// run folder in `discard`, as an open one does.
    Ended(Arc<CaseLawRelay>, ToolsUnavailableReason),
}

impl CaseLaw {
    /// The open relay, if any.
    pub(crate) fn relay(&self) -> Option<&CaseLawRelay> {
        match self {
            CaseLaw::Open(relay) => Some(relay),
            CaseLaw::Absent | CaseLaw::Unavailable(_) | CaseLaw::Ended(..) => None,
        }
    }

    /// Ends an open relay before the launch, for `reason`: its token is revoked now, and it
    /// stops when the run folder goes. Nothing for any other.
    pub(crate) fn end_before_launch(&mut self, reason: ToolsUnavailableReason) {
        if let CaseLaw::Open(relay) = self {
            relay.end();
            *self = CaseLaw::Ended(Arc::clone(relay), reason);
        }
    }
}

/// One session's open relay and its token.
pub(crate) struct CaseLawRelay {
    session: RelaySession,
    /// Kept for the agent's stderr redactor; revoked at the relay with [`end`].
    token: Secret,
}

impl fmt::Debug for CaseLawRelay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaseLawRelay")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl CaseLawRelay {
    /// The relay's port, for `CLAUSEO_RELAY=127.0.0.1:<port>`.
    pub(crate) fn port(&self) -> u16 {
        self.session.addr().port()
    }

    /// Revokes the token and closes the relay's connections: from now on it answers `closed`.
    pub(crate) fn end(&self) {
        self.session.end();
    }

    /// The handle a turn's start and end move.
    fn turns(&self) -> TurnHandle {
        self.session.turns()
    }
}

/// What the launch prep needs for a session's relay, gathered on the actor.
pub(super) struct CaseLawPrep {
    /// The app's relays, when it has them and the hook offers `case-law` for this agent.
    relays: Option<Relays>,
    /// `tools.connectors`.
    switch: Switch,
    /// Claude Code's `managed-mcp.json`, checked on the prep thread.
    managed_mcp: PathBuf,
}

/// The case-law part of `agent`'s launch prep, from the actor's state.
pub(super) fn case_law_prep(actor: &Actor, agent: &AgentId) -> CaseLawPrep {
    let offered = actor.profile.mcp_servers(agent).iter().any(|server| {
        let clauseo_acp::McpServer::Stdio { name, .. } = server;
        name == clauseo_profile::servers::CASE_LAW
    });
    CaseLawPrep {
        relays: actor.wiring.relays.clone().filter(|_| offered),
        switch: Switch {
            on: actor.effective.settings().tools.connectors,
            locked: actor.effective.is_locked("tools.connectors"),
        },
        managed_mcp: actor.managed_mcp.clone(),
    }
}

impl CaseLawPrep {
    /// The session's case law, its relay opened and token file written when it gets one (see
    /// the module header). Keychain, file system and socket calls: off the actor.
    pub(super) fn open(self, run: &RunFolders) -> CaseLaw {
        let Some(relays) = self.relays else {
            return CaseLaw::Absent;
        };
        let status = relays.connector().status();
        if status == CredentialStatus::NotAdded {
            return CaseLaw::Absent;
        }
        if !self.switch.on {
            return CaseLaw::Unavailable(self.switch.off_reason());
        }
        if crate::agent::loads::mcp_control(&self.managed_mcp) == McpControl::Managed {
            return CaseLaw::Unavailable(ToolsUnavailableReason::ManagedByFirm);
        }
        match status {
            CredentialStatus::SignedIn(_) => {}
            CredentialStatus::NotAdded
            | CredentialStatus::SignedOut
            | CredentialStatus::KeyRevoked => return CaseLaw::Absent,
        }
        match open_relay(&relays, run) {
            Ok(relay) => CaseLaw::Open(Arc::new(relay)),
            Err(why) => {
                tracing::warn!(
                    session = %run.key.id(),
                    why,
                    "the session's case-law relay did not open; the session has no case law"
                );
                CaseLaw::Unavailable(ToolsUnavailableReason::NoRelay)
            }
        }
    }
}

/// Binds the port, opens the relay and writes the token file. Each failure names its step,
/// never the token; a relay that opened is stopped again when the file fails.
fn open_relay(relays: &Relays, run: &RunFolders) -> Result<CaseLawRelay, &'static str> {
    if run.relay_token.to_str().is_none() {
        return Err("the token file's path is not UTF-8");
    }
    let listener =
        clauseo_sandbox::ports::bind_loopback().map_err(|_no_port| "no loopback port bound")?;
    let (session, token) = relays
        .open(run.key, listener)
        .map_err(|_not_open| "the relay did not open")?;
    let mut text = token.expose().as_bytes().to_vec();
    text.push(b'\n');
    let written = crate::write::replace_private_file(&run.relay_token, &text);
    text.fill(0);
    if written.is_err() {
        session.stop();
        return Err("the token file was not written");
    }
    Ok(CaseLawRelay { session, token })
}

/// `<root>/relay-token`.
pub(super) fn token_file(root: &Path) -> PathBuf {
    root.join(TOKEN_FILE)
}

/// The secrets `run`'s agent's stderr tail hides beyond its env's: its relay token.
pub(in crate::actor::turn) fn redacted(run: &RunFolders) -> Vec<String> {
    match &run.case_law {
        CaseLaw::Open(relay) | CaseLaw::Ended(relay, _) => vec![relay.token.expose().to_owned()],
        CaseLaw::Absent | CaseLaw::Unavailable(_) => Vec::new(),
    }
}

/// `run`'s session is ending: its relay's token is revoked. Before the run folder goes.
pub(super) fn end(run: &RunFolders) {
    if let Some(relay) = run.case_law.relay() {
        relay.end();
    }
}

/// `thread`'s agent is being stopped: its relay refuses from now on.
pub(in crate::actor::turn) fn end_for(actor: &mut Actor, thread: ThreadId) {
    if let Some(run) = state(actor, thread).and_then(|state| state.run.as_ref()) {
        end(run);
    }
}

/// A turn's prompt goes to `thread`'s agent: the relay's turn caps start again.
pub(in crate::actor::turn) fn turn_started(actor: &mut Actor, thread: ThreadId) {
    if let Some(turns) = turns(actor, thread) {
        turns.start();
    }
}

/// `thread`'s turn ended: calls now count against the between-turns bucket.
pub(in crate::actor::turn) fn turn_ended(actor: &mut Actor, thread: ThreadId) {
    if let Some(turns) = turns(actor, thread) {
        turns.end();
    }
}

fn turns(actor: &mut Actor, thread: ThreadId) -> Option<TurnHandle> {
    let run = state(actor, thread)?.run.as_ref()?;
    Some(run.case_law.relay()?.turns())
}

#[cfg(test)]
mod tests;
