//! The session host's egress proxies (S16): one `clauseo_sandbox` proxy per agent launch that can
//! get the browser, which the host opens on its launch-prep thread and closes once the session's
//! agent tree and Chrome are gone (`clauseo_host::EgressProxy`). [`DesktopProxies`] holds them
//! by session, never by matter: two sessions of one matter get two proxies.
//!
//! Every OS gets it. Tier C (no jail) leaves the agent's own environment alone: only the browser
//! entry reaches its proxy, through the browser port. The quit stops whatever is still open
//! ([`DesktopProxies::stop_all`]) after the host has stopped, waiting at most [`STOP_WAIT`].
//!
//! No lock is held while a proxy starts or stops: a start binds two listeners and spawns a
//! thread, and a stop joins it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_host::{EgressProxy, HostConfig, HostExit, SessionKey};
use clauseo_sandbox::{ProxyConfig, ProxyHandle};
use tauri::{App, AppHandle, Manager as _, Runtime};

use crate::bridge::lock;
use crate::host_start::HostCell;

/// How long the quit waits for the proxies still open to stop before it goes on.
const STOP_WAIT: Duration = Duration::from_secs(3);

/// The open proxies, by session.
#[derive(Debug, Default)]
pub(crate) struct DesktopProxies {
    open: Mutex<BTreeMap<SessionKey, ProxyHandle>>,
}

impl DesktopProxies {
    /// Stops every proxy still open: the quit's, once the host has stopped. They drop on a
    /// thread of their own, each drop joining its proxy's thread, and the quit waits for that
    /// at most [`STOP_WAIT`]; past it, a log line, and the quit goes on.
    pub(crate) fn stop_all(&self) {
        let open = std::mem::take(&mut *lock(&self.open));
        if open.is_empty() {
            return;
        }
        let count = open.len();
        let (done, stopped) = mpsc::channel::<()>();
        let started = std::thread::Builder::new()
            .name("clauseo-egress-stop".to_owned())
            .spawn(move || {
                drop(open);
                let _gone = done.send(());
            });
        // A thread that did not start dropped the proxies here, as it was given back.
        if started.is_ok() && stopped.recv_timeout(STOP_WAIT).is_err() {
            tracing::warn!(
                count,
                wait_s = STOP_WAIT.as_secs(),
                "the egress proxies did not stop in time; the quit goes on"
            );
        }
    }

    /// The sessions with an open proxy.
    #[cfg(test)]
    pub(crate) fn open_sessions(&self) -> Vec<SessionKey> {
        lock(&self.open).keys().copied().collect()
    }

    /// How many proxies are open.
    #[cfg(test)]
    pub(crate) fn open_count(&self) -> usize {
        lock(&self.open).len()
    }
}

impl EgressProxy for DesktopProxies {
    fn open(&self, session: SessionKey, config: ProxyConfig) -> Result<(), clauseo_sandbox::Error> {
        let handle = clauseo_sandbox::start_proxy(config)?;
        // A launch opens once; were one already open for the key, it stops outside the lock.
        let earlier = lock(&self.open).insert(session, handle);
        drop(earlier);
        Ok(())
    }

    fn port_for(&self, session: SessionKey) -> Option<u16> {
        lock(&self.open)
            .get(&session)
            .map(ProxyHandle::browser_port)
    }

    fn close(&self, session: SessionKey) {
        let handle = lock(&self.open).remove(&session);
        // Dropped with the lock released: the drop joins the proxy's thread.
        drop(handle);
    }
}

/// `config` with `proxies` as the host's egress proxy: how the app's host gets it.
pub(crate) fn with_proxies(config: HostConfig, proxies: &Arc<DesktopProxies>) -> HostConfig {
    config.with_egress_proxy(Arc::clone(proxies) as Arc<dyn EgressProxy>)
}

/// The app's proxies, managed so the quit can stop them.
struct EgressState(Arc<DesktopProxies>);

/// New proxies for the app's host, managed in `app` for the quit's [`stop_all`].
pub(crate) fn manage<R: Runtime>(app: &App<R>) -> Arc<DesktopProxies> {
    let proxies = Arc::new(DesktopProxies::default());
    app.manage(EgressState(Arc::clone(&proxies)));
    proxies
}

/// The quit's host step: stops the host (`host_start::stop`), which kills its agents' trees and
/// closes their proxies, then stops any proxy still open, so no proxy thread outlives the host.
/// Says how the host ended.
pub(crate) fn stop_host<R: Runtime>(app: &AppHandle<R>, cell: Option<&HostCell>) -> HostExit {
    let proxies = app.try_state::<EgressState>();
    stop_in_order(
        || cell.map_or(HostExit::Stopped, crate::host_start::stop),
        proxies.as_ref().map(|state| &*state.0),
    )
}

/// [`stop_host`]'s order: `stop` (the host) first, then `proxies`' `stop_all`.
fn stop_in_order(stop: impl FnOnce() -> HostExit, proxies: Option<&DesktopProxies>) -> HostExit {
    let ended = stop();
    if let Some(proxies) = proxies {
        proxies.stop_all();
    }
    ended
}

#[cfg(test)]
mod tests;
