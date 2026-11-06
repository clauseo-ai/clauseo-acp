//! The launch prep's egress part (S16's proxy, R17's seam): the session's proxy, opened on the
//! launch-prep thread beside its browser claims, and closed in the run folder's removal
//! (`discard`), off the actor, once the agent's process tree was killed or the agent never
//! started. A prep nobody waits for closes it too (`undo_late`).
//!
//! A session gets a proxy only when it can get the browser: a proxy is set
//! (`HostConfig::with_egress_proxy`), `tools.browser` is on, a browser slot is free and the
//! profile hook offers a `browser` entry ([`super::browser::BrowserPrep::wanted`]). A proxy
//! that does not open leaves the browser out (`NoProxy`), with a log line; the launch goes on.
//!
//! Tier C (no jail): the agent's own environment is left alone, with no proxy variable, and only
//! the browser entry reaches the proxy, through `{proxy_port}`. The proxy is advisory there:
//! nothing stops the agent going around it until a jail exists (R17 PR-D sets the agent's
//! variables from `ProxyHandle::env` and puts the ports in the policy's `proxy_ports`).
//!
//! What the proxy allows ([`proxy_config`]): the agent kind's built-in hosts
//! (`clauseo_sandbox::builtin_allowlist`), plus the browser's seed sites
//! (`browser::seed_sites`). There is no `network.allowed_domains` setting yet, so no firm list
//! is added. Chrome's background hosts (`browser::deny_hosts`) are refused silently. The
//! upstream is the OS's (`Upstream::System`; R17 PR-D adds `network.upstream_proxy`). Refusals
//! go to [`CountDenials`], which logs a count with the agent and the thread, never the host
//! (decision record 0047); R18 replaces it.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::{AgentId, AgentKind, ThreadId};
use clauseo_sandbox::{
    Allowlist, Denial, DenialSink, LoginKind, ProxyConfig, Upstream, builtin_allowlist,
};

use super::super::super::Actor;
use super::RunFolders;
use super::browser::BrowserPrep;
use crate::browser::deny_hosts::DENY_HOSTS;
use crate::browser::seed_sites::SEED_SITES;
use crate::config::{EgressProxy, SessionKey};

/// A session's open proxy: the proxy that opened it, closed once with [`close`].
#[derive(Clone)]
pub(crate) struct Egress {
    proxy: Arc<dyn EgressProxy>,
}

impl fmt::Debug for Egress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Egress").finish_non_exhaustive()
    }
}

impl Egress {
    /// The session's browser listener port, as the proxy answers it.
    pub(crate) fn port_for(&self, key: SessionKey) -> Option<u16> {
        self.proxy.port_for(key)
    }
}

/// What the launch prep needs for a session's proxy, gathered on the actor.
pub(super) struct EgressPrep {
    /// The proxy, when one is set and the session can get the browser.
    proxy: Option<Arc<dyn EgressProxy>>,
    /// The agent, for its built-in hosts and the denial log.
    agent: AgentId,
}

/// The egress part of `agent`'s launch prep: a proxy to open only when `browser` will look for
/// the session's browser.
pub(super) fn egress_prep(actor: &Actor, agent: &AgentId, browser: &BrowserPrep) -> EgressPrep {
    EgressPrep {
        proxy: actor.wiring.proxy.clone().filter(|_| browser.wanted()),
        agent: agent.clone(),
    }
}

impl EgressPrep {
    /// Opens `key`'s proxy, when the session gets one. Starts a thread and binds two loopback
    /// listeners: off the actor.
    pub(super) fn open(self, key: SessionKey) -> Option<Egress> {
        let proxy = self.proxy?;
        match proxy.open(key, proxy_config(&self.agent, key.thread)) {
            Ok(()) => Some(Egress { proxy }),
            Err(error) => {
                // The kind only: a config error's text names the host it refused.
                let why = if matches!(error, clauseo_sandbox::Error::ProxyStart(_)) {
                    "the proxy did not start"
                } else {
                    "the proxy's config was refused"
                };
                tracing::warn!(
                    session = %key.id(),
                    why,
                    "the session's egress proxy did not open; the session has no browser"
                );
                None
            }
        }
    }
}

/// `run`'s session has ended (its agent's tree killed) or never started: its proxy, if it has
/// one, is closed now. Blocks while the proxy's thread is joined: off the actor.
pub(super) fn close(run: &mut RunFolders) {
    if let Some(egress) = run.egress.take() {
        egress.proxy.close(run.key);
    }
}

/// The proxy config for `agent`'s session on `thread` (see the module header).
pub(crate) fn proxy_config(agent: &AgentId, thread: ThreadId) -> ProxyConfig {
    // The plan sign-in's hosts hold the API-key sign-in's: the session's real sign-in is read
    // from its final environment in R17 PR-D.
    let builtin = match AgentKind::of(agent) {
        Some(AgentKind::Claude) => builtin_allowlist(&LoginKind::ClaudePlan),
        Some(AgentKind::Codex) => builtin_allowlist(&LoginKind::CodexChatGpt),
        None => Allowlist::default(),
    };
    ProxyConfig {
        allow: builtin.union(hosts(SEED_SITES.iter().copied())),
        silent: hosts(DENY_HOSTS.iter().map(|deny| deny.host)),
        upstream: Upstream::System,
        denials: Arc::new(CountDenials {
            agent: agent.clone(),
            thread,
            count: AtomicU64::new(0),
        }),
        record: None,
    }
}

/// One list of `names`, each an exact host. A name the proxy cannot read is left out, with a
/// log line that does not name it.
fn hosts<'a>(names: impl Iterator<Item = &'a str>) -> Allowlist {
    names.fold(Allowlist::default(), |list, name| {
        if let Ok(one) = Allowlist::exact(name) {
            list.union(one)
        } else {
            tracing::warn!("egress: a listed host is not a host name; left out");
            list
        }
    })
}

/// The session's denial sink until R18's: it counts the proxy's refusals and logs each with the
/// count, the agent and the thread, never the URL or the host (decision record 0047). It never
/// blocks: a counter and a log line.
pub(crate) struct CountDenials {
    agent: AgentId,
    thread: ThreadId,
    count: AtomicU64,
}

impl CountDenials {
    /// The refusals so far.
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

impl DenialSink for CountDenials {
    fn denied(&self, d: Denial) {
        // The rate cap's summary stands for that many refusals.
        let refused = if let Denial::Dropped { count } = d {
            u64::from(count)
        } else {
            1
        };
        let count = self
            .count
            .fetch_add(refused, Ordering::Relaxed)
            .saturating_add(refused);
        tracing::info!(
            agent = self.agent.as_str(),
            thread = %self.thread,
            count,
            "egress: the session's proxy refused a request"
        );
    }
}

#[cfg(test)]
mod tests;
