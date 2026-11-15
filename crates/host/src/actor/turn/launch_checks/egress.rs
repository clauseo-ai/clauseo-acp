//! The launch prep's egress part (S16's proxy, R17's seam): the session's proxy, opened on the
//! launch-prep thread once its Chrome and browser claims are found, and held by an
//! [`EgressGuard`] on the session's `RunFolders`. The guard closes the proxy when it drops: in
//! the run folder's removal (`discard`), off the actor and after the agent's tree and its Chrome
//! are gone (`straggler`), at host stop for a session held past that wait, or wherever a panic
//! or a dropped task lets go of the run first, so nothing leaks the proxy.
//!
//! A session gets a proxy only when it can get the browser: a proxy is set
//! (`HostConfig::with_egress_proxy`), `tools.browser` is on, a browser slot is free and the
//! profile hook offers a `browser` entry that asks for the proxy (`{proxy_port}`, which Codex's
//! entry holds; [`super::browser::BrowserPrep::wanted`]), and then only once its sidecars,
//! Chrome and ports were found, so a `BrowserNotInstalled` session holds no proxy. A proxy that
//! does not open leaves the browser out (`NoProxy`), with a log line; the launch goes on. One
//! that refuses to start in open mode because no block list is loaded leaves it out with
//! `BlockListNotLoaded` (fail closed, ADR 0053): the next session launched gets it back. Both
//! listeners bind outside Windows' 100-port tier-B range (`ListenerFor::Other`: a port the OS
//! picks), so that range does not bound how many sessions have a proxy.
//!
//! Tier C (no jail): the agent's own environment is left alone, with no proxy variable, and only
//! the browser entry reaches the proxy, through `{proxy_port}` and the browser listener's
//! password ([`EgressGuard::secret`]), which the guard gives Chrome over CDP. The password goes
//! to the guard in a file, `<run>/proxy-secret` ([`write_secret`]), which the entry names
//! (`{proxy_secret_file}`) and the guard deletes once read: never in the entry, whose variables
//! the Claude Agent SDK puts on a command line and Codex receives in `session/new`.
//! Without the password the proxy answers 407, so a process that reaches loopback (Codex's
//! sandboxed shell on Windows) cannot use it. The proxy is advisory for the agent itself:
//! nothing stops the agent going around it until a jail exists (R17 PR-D sets the agent's
//! variables from `ProxyHandle::env` and puts the ports in the policy's `proxy_ports`).
//!
//! What the proxy allows ([`proxy_config`]): open mode (`ProxyMode::Open`, ADR 0053), any
//! public host on port 443 (CONNECT) or 80 (plain http), minus the unsafe-site block list and
//! every loopback, private, link-local, unique-local, CGNAT or multicast address, checked after
//! DNS against the address connected to. Its allowlist (the agent kind's built-in hosts,
//! `clauseo_sandbox::builtin_allowlist`, plus the browser's seed sites, `browser::seed_sites`)
//! no longer decides anything in open mode. Chrome's background hosts (`browser::deny_hosts`)
//! are refused silently. The upstream is the OS's (`Upstream::System`; R17 PR-D adds
//! `network.upstream_proxy`). The block list (0053) is the shared `Current` of the host's
//! `BlockListSeams`, the one its updater swaps; the proxy reads it per request, so a swap
//! reaches the next connection. With no seams the list is empty. Refusals go to
//! [`CountDenials`], which counts them on the proxy's thread and posts each `Denial::Blocked` to
//! the actor (`actor/blocked.rs`, the app's notice); the guard logs the session's total when
//! the proxy closes, never a host (decision record 0047).
//! R18 replaces it.

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_blocklist::{Allowed, Current};
use clauseo_protocol::{AgentId, AgentKind, ToolAvailability, ToolState, ToolsUnavailableReason};
use clauseo_sandbox::ports::ListenerFor;
use clauseo_sandbox::{
    Allowlist, Denial, DenialSink, LoginKind, ProxyConfig, ProxyCredential, ProxyMode, Upstream,
    builtin_allowlist,
};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::RunFolders;
use super::browser::BrowserPrep;
use crate::agent::loads::McpControl;
use crate::blocklist::{AllowedSites, FirstLoad};
use crate::browser::deny_hosts::DENY_HOSTS;
use crate::browser::seed_sites::SEED_SITES;
use crate::clock::Clock;
use crate::config::{EgressProxy, SessionKey};
use crate::handle::BlockedSite;

/// A session's open proxy. Dropping it closes the proxy (`EgressProxy::close`, which may block
/// while the proxy's thread is joined) and logs the session's refusals. It sits last on
/// `RunFolders`, so the run's browser claims and relay drop before it.
pub(crate) struct EgressGuard {
    proxy: Arc<dyn EgressProxy>,
    key: SessionKey,
    denials: Arc<CountDenials>,
    /// The browser listener's password, read once when the proxy opened; `Debug` hides it.
    credential: Option<ProxyCredential>,
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

    /// The session's browser listener password (`<run>/proxy-secret`), as the proxy gave it when it
    /// opened. For the `browser` entry and the agent's stderr redactor only; never logged.
    pub(crate) fn secret(&self) -> Option<&ProxyCredential> {
        self.credential.as_ref()
    }
}

/// The password file's name in the session's run folder.
pub(crate) const SECRET_FILE: &str = "proxy-secret";

/// `<root>/proxy-secret`.
pub(super) fn secret_file(root: &Path) -> PathBuf {
    root.join(SECRET_FILE)
}

/// Writes `run`'s proxy password, when its proxy opened with one, to `<run>/proxy-secret`
/// through `write::replace_private_file` (0600 on Unix, in the 0700 run folder; on Windows it
/// inherits the run folder's ACL, as the relay token does, decision D8): 64 hex characters and a
/// line end, read once and deleted by the browser guard. A path that is not UTF-8 (no entry
/// could name it) or a file not written closes the proxy again, so the browser is left out with
/// `NoProxy`; a log line names the step, never the password.
pub(super) fn write_secret(run: &mut RunFolders) {
    let Some(credential) = run.egress.as_ref().and_then(|egress| egress.secret()) else {
        return;
    };
    let why = if run.proxy_secret.to_str().is_none() {
        Some("the password file's path is not UTF-8")
    } else {
        let mut text = credential.password().as_bytes().to_vec();
        text.push(b'\n');
        let written = crate::write::replace_private_file(&run.proxy_secret, &text);
        text.fill(0);
        written
            .err()
            .map(|_not_written| "the password file was not written")
    };
    if let Some(why) = why {
        tracing::warn!(
            session = %run.key.id(),
            why,
            "the session's egress proxy has no password file; the session has no browser"
        );
        run.egress = None;
    }
}

/// `run`'s password file, once written: none when the proxy did not open, or the file was not
/// written (which closed the proxy again, [`write_secret`]).
pub(in crate::actor::turn) fn written_secret(run: &RunFolders) -> Option<&Path> {
    let password = run.egress.as_ref().and_then(|egress| egress.secret());
    password.map(|_password| run.proxy_secret.as_path())
}

/// The browser entry was left out after the proxy opened (`state`, a managed MCP config that
/// appeared since the launch prep, or a path that is not UTF-8): no guard will read the file,
/// so it is removed off the actor. The proxy stays open; its password is then in its memory
/// alone, where nothing can use it.
pub(in crate::actor::turn) fn unread_secret(run: &RunFolders, state: Option<&ToolState>) {
    let available = state.is_some_and(|state| state.availability == ToolAvailability::Available);
    if let (false, Some(file)) = (available, written_secret(run)) {
        let file = file.to_path_buf();
        // Never joined: a remove that fails leaves the file to the run folder's removal.
        drop(std::thread::spawn(move || {
            drop(crate::write::remove_old_file(&file));
        }));
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
    /// The block list's first load, to wait for before the proxy opens: only with a proxy and
    /// a bundled snapshot (`blocklist/first_load.rs`).
    first_load: Option<FirstLoad>,
    /// The allowed sites' views; the launch takes its matter's when its proxy opens.
    allowed: Arc<AllowedSites>,
    /// The firm's `managed-mcp.json`, read on the launch-prep thread before the proxy opens.
    managed_mcp: PathBuf,
}

/// The egress part of `agent`'s launch prep: a proxy to open only when `browser` will look for
/// the session's browser.
pub(super) fn egress_prep(actor: &Actor, agent: &AgentId, browser: &BrowserPrep) -> EgressPrep {
    // The shell tool has no proxy: only an entry that asks for one (`{proxy_port}`, Codex's
    // `browser` entry) reaches the session's proxy.
    let proxy = actor
        .wiring
        .proxy
        .clone()
        .filter(|_| browser.wanted() && !browser.shell() && browser.proxy());
    let seams = actor.wiring.blocklist.as_ref();
    EgressPrep {
        first_load: seams
            .filter(|seams| proxy.is_some() && seams.bundled.is_some())
            .map(|seams| seams.first_load.clone()),
        proxy,
        agent: agent.clone(),
        block: seams.map(|seams| Arc::clone(seams.current())),
        post: actor.post.clone(),
        allowed: Arc::clone(actor.allowed.sites()),
        managed_mcp: actor.managed_mcp.clone(),
    }
}

impl EgressPrep {
    /// The wait for the block list's first load, at most `FIRST_LOAD_WAIT` on `clock`, when
    /// this launch's proxy should wait for it; the launch prep awaits it before its thread.
    /// Boxed: the launch's future stays small (`large_futures` on Windows).
    pub(super) fn block_list_wait(
        &self,
        clock: Arc<dyn Clock>,
    ) -> Option<Pin<Box<dyn Future<Output = ()>>>> {
        let first_load = self.first_load.clone()?;
        let current = self.block.clone()?;
        Some(Box::pin(first_load.wait(clock, current)))
    }

    /// Opens `key`'s proxy, when the session gets one, its browser listener (which takes the
    /// password [`EgressGuard::secret`] holds) bound only when the launch offers the browser
    /// (`browser`, `ProxyConfig::browser`). Under tier C a launch without the browser gets no
    /// proxy at all: nothing would go through it. The jail
    /// (R17 PR-D) opens one for every session, with `browser` false for those. Starts a thread
    /// and binds loopback listeners: off the actor. The second value is the browser's reason
    /// when the proxy did not open for one with its own words: `BlockListNotLoaded`, when open
    /// mode refused to start with no block list loaded (the bundled snapshot failed its check,
    /// or none was bundled and no fetch succeeded yet). The next session launched finds the
    /// list the updater loaded since and gets the browser; a running session keeps none.
    pub(super) fn open(
        self,
        key: SessionKey,
        browser: bool,
    ) -> (Option<Arc<EgressGuard>>, Option<ToolsUnavailableReason>) {
        let Some(proxy) = self.proxy.filter(|_| browser) else {
            return (None, None);
        };
        // The firm's `managed-mcp.json` leaves the entry out (`ManagedByFirm`): no proxy, and
        // so no password file nobody reads.
        if crate::agent::loads::mcp_control(&self.managed_mcp) == McpControl::Managed {
            return (None, None);
        }
        let denials = Arc::new(CountDenials::reporting(key, self.post));
        match proxy.open(
            key,
            proxy_config(
                &self.agent,
                Arc::clone(&denials),
                (self.block, self.allowed.for_matter(key.matter)),
                browser,
            ),
        ) {
            Ok(()) => {
                let credential = proxy.credential_for(key);
                let guard = EgressGuard {
                    proxy,
                    key,
                    denials,
                    credential,
                };
                (Some(Arc::new(guard)), None)
            }
            Err(error) => {
                // The kind only: a config error's text names the host it refused.
                let (why, reason) = if matches!(error, clauseo_sandbox::Error::BlockListNotLoaded) {
                    (
                        "no block list is loaded",
                        Some(ToolsUnavailableReason::BlockListNotLoaded),
                    )
                } else if matches!(error, clauseo_sandbox::Error::ProxyStart(_)) {
                    ("the proxy did not start", None)
                } else {
                    ("the proxy's config was refused", None)
                };
                tracing::warn!(
                    session = %key.id(),
                    why,
                    "the session's egress proxy did not open; the session has no browser"
                );
                (None, reason)
            }
        }
    }
}

/// The proxy config for `agent`'s session, its refusals counted in `denials` (see the module
/// header), refusing the hosts on `block` (an empty list when `None`) but those on `allowed`
/// (the session's matter's live view, `AllowedSites::for_matter`), with the browser listener
/// when the launch offers the browser (`browser`).
pub(crate) fn proxy_config(
    agent: &AgentId,
    denials: Arc<CountDenials>,
    (block, allowed): (Option<Arc<Current>>, Arc<Allowed>),
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
        // Held by the proxy, and with it its revoke watcher, for the session's life: the proxy
        // lives until `EgressGuard` closes it at the run's removal.
        allowed,
        upstream: Upstream::System,
        denials,
        record: None,
        // Stage C's rule: the browser listener only for a launch with the browser.
        browser,
        // ADR 0053: any public host but the listed and private ones, so the block list is what
        // holds the browser back.
        mode: ProxyMode::Open,
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
