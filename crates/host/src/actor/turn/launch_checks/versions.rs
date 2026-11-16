//! The launch prep's version-history part: the session's second loopback listener
//! (`connectors::VersionsRelay`), opened on the launch-prep thread beside the case-law relay, and
//! its token file. Unlike the relay it does not wait on case law: it opens for every session
//! whose profile hook offers a `clauseo-versions` entry for the agent, unless the firm's
//! `managed-mcp.json` is present (no Clauseo server goes then), and nothing is bound and no file
//! is written otherwise.
//!
//! The backend (`crate::versions`) is built here per launch on the host's own source
//! (`versions::source::HostSource`: the actor's channel and the store), for the session's matter
//! only. The token goes to `<run>/versions-token` (`run_tokens::VERSIONS_TOKEN`) through
//! `write::replace_private_file`, as the relay's does, and to the agent's stderr redactor; never
//! into the entry, an argument, a variable or a log. When the session ends, [`end`] revokes the
//! token before the run folder, and the file with it, is removed; the listener itself stops when
//! the last [`RunFolders`] clone drops, after the agent's tree was killed.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_connectors::Secret;
use clauseo_protocol::{AgentId, ThreadId};
use clauseo_sandbox::ports::ListenerFor;

use super::super::super::Actor;
use super::super::state;
use super::RunFolders;
use crate::agent::loads::McpControl;
use crate::connectors::VersionsRelay;
use crate::versions::source::HostSource;
use crate::versions::{VersionsBackend, VersionsConfig, VersionsTurns};

/// What the launch prep made of the session's version history.
#[derive(Debug, Clone, Default)]
pub(crate) enum Versions {
    /// No listener: the hook offers no entry, the firm's `managed-mcp.json` is present, or it
    /// did not open (logged).
    #[default]
    Absent,
    /// The listener is open and the token file written. Its token is revoked when its entry
    /// does not go (`actor/turn/profile_loads/own_servers.rs`) and when the session ends
    /// ([`end`]); it stops when the run folder goes.
    Open(Arc<VersionsListener>),
}

impl Versions {
    /// The open listener, if any.
    pub(crate) fn listener(&self) -> Option<&VersionsListener> {
        match self {
            Versions::Open(listener) => Some(listener),
            Versions::Absent => None,
        }
    }
}

/// One session's open listener and its token.
pub(crate) struct VersionsListener {
    relay: VersionsRelay,
    /// Kept for the agent's stderr redactor; revoked at the listener with [`end`].
    token: Secret,
}

impl fmt::Debug for VersionsListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionsListener")
            .field("relay", &self.relay)
            .finish_non_exhaustive()
    }
}

impl VersionsListener {
    /// The listener's port, for `CLAUSEO_RELAY=127.0.0.1:<port>`.
    pub(crate) fn port(&self) -> u16 {
        self.relay.port()
    }

    /// Revokes the token and closes the listener's connections.
    pub(crate) fn end(&self) {
        self.relay.end();
    }

    fn turns(&self) -> VersionsTurns {
        self.relay.turns()
    }
}

/// What the launch prep needs for a session's listener, gathered on the actor.
pub(super) struct VersionsPrep {
    /// The session's backend, when the hook offers `clauseo-versions` for this agent.
    backend: Option<VersionsBackend>,
    /// The host's clock, for the handshake deadline and the accept loop's pauses.
    clock: Arc<dyn crate::clock::Clock>,
    /// Claude Code's `managed-mcp.json`, checked on the prep thread.
    managed_mcp: PathBuf,
}

/// The version-history part of `agent`'s launch prep for `run`'s session, from the actor.
pub(super) fn versions_prep(actor: &Actor, agent: &AgentId, run: &RunFolders) -> VersionsPrep {
    let offered = actor
        .profile
        .mcp_servers(agent)
        .iter()
        .any(|server| server.name() == clauseo_profile::servers::VERSIONS);
    let backend = offered.then(|| {
        VersionsBackend::new(VersionsConfig {
            session: run.key,
            source: Arc::new(HostSource::new(actor.post.clone(), actor.store.clone())),
            clock: Arc::clone(&actor.clock),
            data_dir: actor.effective.data_dir().to_path_buf(),
            stetdoc: actor.wiring.stetdoc.as_ref().map(|s| s.program.clone()),
        })
    });
    VersionsPrep {
        backend,
        clock: Arc::clone(&actor.clock),
        managed_mcp: actor.managed_mcp.clone(),
    }
}

impl VersionsPrep {
    /// The session's listener, opened and its token file written when it gets one (see the
    /// module header). File system and socket calls: off the actor.
    pub(super) fn open(self, run: &RunFolders) -> Versions {
        let Some(backend) = self.backend else {
            return Versions::Absent;
        };
        if crate::agent::loads::mcp_control(&self.managed_mcp) == McpControl::Managed {
            return Versions::Absent;
        }
        match open_listener(backend, self.clock, run) {
            Ok(listener) => Versions::Open(Arc::new(listener)),
            Err(why) => {
                tracing::warn!(
                    session = %run.key.id(),
                    why,
                    "the session's version-history listener did not open; the session has no \
                     version-history tool"
                );
                Versions::Absent
            }
        }
    }
}

/// Binds the port, opens the listener and writes the token file. Each failure names its step,
/// never the token; a listener that opened is stopped again when the file fails.
fn open_listener(
    backend: VersionsBackend,
    clock: Arc<dyn crate::clock::Clock>,
    run: &RunFolders,
) -> Result<VersionsListener, &'static str> {
    if run.versions_token.to_str().is_none() {
        return Err("the token file's path is not UTF-8");
    }
    // Tier C, as the relay's (`relay.rs`): R17 PR-E passes `ListenerFor::TierB` for a tier-B
    // launch, whose budget counts this port (`clauseo_sandbox::ports`).
    let listener = clauseo_sandbox::ports::bind_loopback(ListenerFor::Other)
        .map_err(|_no_port| "no loopback port bound")?;
    let (relay, token) = VersionsRelay::open(run.key, backend, clock, listener)
        .map_err(|_not_open| "the listener did not open")?;
    let mut text = token.expose().as_bytes().to_vec();
    text.push(b'\n');
    let written = crate::write::replace_private_file(&run.versions_token, &text);
    text.fill(0);
    if written.is_err() {
        drop(relay);
        return Err("the token file was not written");
    }
    Ok(VersionsListener { relay, token })
}

/// `<root>/versions-token`.
pub(super) fn token_file(root: &Path) -> PathBuf {
    root.join(crate::run_tokens::VERSIONS_TOKEN)
}

/// The listener's token, for `run`'s agent's stderr redactor; none without a listener.
pub(in crate::actor::turn) fn redacted(run: &RunFolders) -> Option<String> {
    match &run.versions {
        Versions::Open(listener) => Some(listener.token.expose().to_owned()),
        Versions::Absent => None,
    }
}

/// `run`'s session is ending: its listener's token is revoked. Before the run folder goes.
pub(super) fn end(run: &RunFolders) {
    if let Some(listener) = run.versions.listener() {
        listener.end();
    }
}

/// `thread`'s agent is being stopped: its listener refuses from now on.
pub(in crate::actor::turn) fn end_for(actor: &mut Actor, thread: ThreadId) {
    if let Some(run) = state(actor, thread).and_then(|state| state.run.as_ref()) {
        end(run);
    }
}

/// A turn's prompt goes to `thread`'s agent: the tool's per-turn caps start again.
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

fn turns(actor: &mut Actor, thread: ThreadId) -> Option<VersionsTurns> {
    let run = state(actor, thread)?.run.as_ref()?;
    Some(run.versions.listener()?.turns())
}
