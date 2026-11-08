//! The launch prep's egress part (S16's proxy, R17's seam): the session's proxy, opened on the
//! launch-prep thread once its Chrome and browser claims are found, and held by an
//! [`EgressGuard`] on the session's `RunFolders`. The guard closes the proxy when it drops: in
//! the run folder's removal (`discard`), off the actor and after the agent's tree and its Chrome
//! are gone (`straggler`), at host stop for a session held past that wait, or wherever a panic
//! or a dropped task lets go of the run first, so nothing leaks the proxy.
//!
//! A session gets a proxy only when it can get the browser: a proxy is set
//! (`HostConfig::with_egress_proxy`), `tools.browser` is on, a browser slot is free and the
//! profile hook offers a `browser` entry ([`super::browser::BrowserPrep::wanted`]), and then
//! only once its sidecars, Chrome and ports were found, so a `BrowserNotInstalled` session holds
//! none of the proxy's ports (Windows' range is 100). A proxy that does not open leaves the
//! browser out (`NoProxy`), with a log line; the launch goes on.
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
//! upstream is the OS's (`Upstream::System`; R17 PR-D adds `network.upstream_proxy`). The
//! block list (0053) is the shared `Current` of the host's `BlockListSeams`, the one its
//! updater swaps; the proxy reads it per request, so a swap reaches the next connection. With no
//! seams the list is empty. Refusals go to [`CountDenials`], which counts them on the proxy's
//! thread and posts each `Denial::Blocked` to the actor (`actor/blocked.rs`, the app's notice);
//! the guard logs the session's total when the proxy closes, never a host (decision record 0047).
//! R18 replaces it.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_blocklist::Current;
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_sandbox::ports::ListenerFor;
use clauseo_sandbox::{
    Allowlist, Denial, DenialSink, LoginKind, ProxyConfig, Upstream, builtin_allowlist,
};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::browser::BrowserPrep;
use crate::browser::deny_hosts::DENY_HOSTS;
use crate::browser::seed_sites::SEED_SITES;
use crate::config::{EgressProxy, SessionKey};
use crate::handle::BlockedSite;

/// A session's open proxy. Dropping it closes the proxy (`EgressProxy::close`, which may block
/// while the proxy's thread is joined) and logs the session's refusals. It sits last on
/// `RunFolders`, so the run's browser claims and relay drop before it.
pub(crate) struct EgressGuard {
    proxy: Arc<dyn EgressProxy>,
    key: SessionKey,
    denials: Arc<CountDenials>,
}

impl fmt::Debug for EgressGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EgressGuard")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl EgressGuard {
    /// The session's browser listener port, as the proxy answers it.
    pub(crate) fn port(&self) -> Option<u16> {
        self.proxy.port_for(self.key)
    }
}

impl Drop for EgressGuard {
    fn drop(&mut self) {
        self.proxy.close(self.key);
        tracing::info!(
            matter = %self.key.matter,
            thread = %self.key.thread,
            launch = self.key.launch,
            refused = self.denials.count(),
            "egress: the session's proxy closed"
        );
    }
}

/// What the launch prep needs for a session's proxy, gathered on the actor.
pub(super) struct EgressPrep {
    /// The proxy, when one is set and the session can get the browser.
    proxy: Option<Arc<dyn EgressProxy>>,
    /// The agent, for its built-in hosts.
    agent: AgentId,
    /// The block list in use (`BlockListSeams::current`), when the host has one.
    block: Option<Arc<Current>>,
    /// The actor's input channel, for the sink's blocked-site reports.
    post: async_channel::Sender<Input>,
}

/// The egress part of `agent`'s launch prep: a proxy to open only when `browser` will look for
/// the session's browser.
pub(super) fn egress_prep(actor: &Actor, agent: &AgentId, browser: &BrowserPrep) -> EgressPrep {
    EgressPrep {
        proxy: actor.wiring.proxy.clone().filter(|_| browser.wanted()),
        agent: agent.clone(),
        block: actor
            .wiring
            .blocklist
            .as_ref()
            .map(|seams| Arc::clone(seams.current())),
        post: actor.post.clone(),
    }
}

impl EgressPrep {
    /// Opens `key`'s proxy, when the session gets one, its token-free browser listener bound
    /// only when the launch offers the browser (`browser`, `ProxyConfig::browser`). Under tier C
    /// a launch without the browser gets no proxy at all: nothing would go through it. The jail
    /// (R17 PR-D) opens one for every session, with `browser` false for those. Starts a thread
    /// and binds loopback listeners: off the actor.
    pub(super) fn open(self, key: SessionKey, browser: bool) -> Option<Arc<EgressGuard>> {
        let proxy = self.proxy.filter(|_| browser)?;
        let denials = Arc::new(CountDenials::reporting(key, self.post));
        match proxy.open(
            key,
            proxy_config(&self.agent, Arc::clone(&denials), self.block, browser),
        ) {
            Ok(()) => Some(Arc::new(EgressGuard {
                proxy,
                key,
                denials,
            })),
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

/// The proxy config for `agent`'s session, its refusals counted in `denials` (see the module
/// header), refusing the hosts on `block` (an empty list when `None`), with the browser
/// listener when the launch offers the browser (`browser`).
pub(crate) fn proxy_config(
    agent: &AgentId,
    denials: Arc<CountDenials>,
    block: Option<Arc<Current>>,
    browser: bool,
) -> ProxyConfig {
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
        // The shared list the updater swaps (0053), read per request; none loaded without seams.
        block: block.unwrap_or_default(),
        // None allowed yet: the lawyer's allowed sites reach the session's proxy in R17 PR-D.
        allowed: Arc::default(),
        upstream: Upstream::System,
        denials,
        record: None,
        // Stage C's rule: the token-free listener only for a launch with the browser.
        browser,
        // Tier C (the module header): the agent listener stays outside `TIER_B_PORTS` until
        // R17 PR-E passes `ListenerFor::TierB` for a tier-B launch, as for the relay.
        listener: ListenerFor::Other,
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

/// The session's denial sink until R18's. It runs on the proxy's thread, so it only adds to an
/// atomic counter and posts each block-list refusal on the actor's unbounded channel
/// (`try_send`, which never waits): no log, no lock, nothing that blocks. [`EgressGuard`] logs
/// the total.
#[derive(Debug, Default)]
pub(crate) struct CountDenials {
    count: AtomicU64,
    /// Where a `Denial::Blocked` goes: the session and the actor's channel.
    blocked: Option<(SessionKey, async_channel::Sender<Input>)>,
}

impl CountDenials {
    /// A sink that also reports each `Denial::Blocked` for `session` to the actor.
    pub(crate) fn reporting(session: SessionKey, post: async_channel::Sender<Input>) -> Self {
        Self {
            count: AtomicU64::new(0),
            blocked: Some((session, post)),
        }
    }

    /// The refusals so far.
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
        self.count.fetch_add(refused, Ordering::Relaxed);
        if let (
            Denial::Blocked {
                host,
                tag,
                generated,
                ..
            },
            Some((session, post)),
        ) = (d, &self.blocked)
        {
            // The host only (never a path). The internal channel is unbounded, which is fine: the
            // proxy passes on at most 20 refusals a second per session (S14's rate cap; more come
            // as one `Denial::Dropped`), each host name is at most 253 bytes, and the 403 was
            // decided before this send, so nothing here holds up the refusal. A failed send
            // happens only once the host has stopped, and the report is then dropped.
            let site = BlockedSite {
                host,
                tag,
                generated,
            };
            let _gone = post.try_send(Input::SiteBlocked {
                session: *session,
                site,
            });
        }
    }
}

#[cfg(test)]
mod tests;
