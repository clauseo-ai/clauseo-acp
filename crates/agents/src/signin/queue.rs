//! The queued start: a sign-in pressed before the agent's install is `Ready`, or while offline,
//! reports `Queued` and starts by itself once both hold.
//!
//! - The install: the job reads the shared install state, and while it is not `Ready` it waits
//!   for its slot's kick, which `Shared` sends on every `Ready` (`SigninHub::install_ready`).
//!   An install that ends `Failed` or `Waiting` sends none, so the sign-in stays `Queued` until
//!   a later install succeeds or the sign-in is cancelled or superseded.
//! - The network: the agent's first sign-in host (or, with a proxy set, the proxy's host, which
//!   is the name the install's downloads and the CLI look up then) must resolve, through the
//!   install's resolver seam ([`crate::InstallConfig::resolver`] under test-support, else the
//!   OS). A failed lookup is offline: `Queued`, then again every 30 s on the timer, the install's
//!   offline cadence. The lookup is the only network call.
//!
//! The order does not matter: the install is checked first, then the lookup, and every wake
//! checks both again. `Queued` is sent once, the first time either fails; a cancel while queued
//! ends the job with `Failed { code: Cancelled }`, and so does one that lands during the lookup,
//! which is checked again once the lookup returns.

use std::net::ToSocketAddrs;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::Timer;
use futures::future::{Either, select};

use crate::config::AgentsConfig;
use crate::signin::SigninState;
use crate::state::Installed;
use crate::worker::{Shared, Ticket};

/// The wait between lookups while offline: the install's `OFFLINE_WAIT`.
pub(crate) const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// What the wait needs.
pub(crate) struct Queue<'a, T> {
    pub(crate) ticket: &'a Ticket,
    pub(crate) config: &'a AgentsConfig,
    pub(crate) shared: &'a Arc<Shared>,
    pub(crate) timer: &'a T,
    /// The slot's kick: a message on each install `Ready`; closed once the slot is gone.
    pub(crate) kick: &'a async_channel::Receiver<()>,
}

/// How the wait ended.
pub(crate) enum Gate {
    /// Both hold: the run starts with this install.
    Start(Installed),
    /// Cancelled with `cancel_sign_in`, superseded, or the handle is gone.
    Stop,
}

/// Waits until the agent's install is `Ready` and its sign-in host resolves, sending `Queued`
/// the first time either does not.
pub(crate) async fn until_startable<T: Timer>(queue: &Queue<'_, T>) -> Gate {
    let ticket = queue.ticket;
    let mut said = false;
    let mut say_queued = || {
        if !said {
            said = true;
            queue
                .shared
                .signin
                .emit_job(ticket, SigninState::Queued, false);
        }
    };
    loop {
        if ticket.cancel.is_cancelled() || !queue.shared.signin.is_job(ticket) {
            return Gate::Stop;
        }
        let Some(installed) = queue.shared.installed(&ticket.agent) else {
            say_queued();
            let kicked = pin!(queue.kick.recv());
            match select(kicked, pin!(ticket.cancel.cancelled())).await {
                Either::Left((Ok(()), _)) => continue,
                // The slot is gone (only ever with its cancel set) or the job was cancelled.
                Either::Left((Err(_), _)) | Either::Right(_) => return Gate::Stop,
            }
        };
        let online = online(queue.config, &ticket.agent);
        // The lookup may block for as long as the OS takes: a cancel or a newer job that
        // landed meanwhile ends the wait here, before any `Waiting` or run.
        if ticket.cancel.is_cancelled() || !queue.shared.signin.is_job(ticket) {
            return Gate::Stop;
        }
        if online {
            return Gate::Start(installed);
        }
        say_queued();
        let wait = pin!(queue.timer.after(OFFLINE_WAIT));
        if let Either::Right(_) = select(wait, pin!(ticket.cancel.cancelled())).await {
            return Gate::Stop;
        }
    }
}

/// Whether the name `agent`'s sign-in needs first resolves: the proxy's host when a proxy is set,
/// else the snapshot's first sign-in host. An agent with neither has nothing to look up.
fn online(config: &AgentsConfig, agent: &clauseo_protocol::AgentId) -> bool {
    let proxy = config
        .install
        .proxy
        .as_deref()
        .and_then(|proxy| ureq::Proxy::new(proxy).ok());
    let (host, port) = if let Some(proxy) = &proxy {
        (proxy.host().to_owned(), proxy.port())
    } else {
        let first = config
            .snapshot
            .agent(agent)
            .and_then(|entry| entry.signin_hosts.first().cloned());
        let Some(host) = first else {
            return true;
        };
        (host, 443)
    };
    resolves(config, &host, port)
}

/// One lookup of `host`, through the test resolver when one is set, else the OS's.
fn resolves(config: &AgentsConfig, host: &str, port: u16) -> bool {
    #[cfg(feature = "test-support")]
    if let Some(resolver) = &config.install.resolver {
        return (resolver.0)(host).is_ok_and(|addrs| !addrs.is_empty());
    }
    let _ = config;
    (host, port)
        .to_socket_addrs()
        .is_ok_and(|mut addrs| addrs.next().is_some())
}
