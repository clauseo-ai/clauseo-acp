//! The session host's egress proxies (S16): one `clauseo_sandbox` proxy per agent launch that can
//! get the browser, which the host opens on its launch-prep thread and closes once the session's
//! agent tree has exited (`clauseo_host::EgressProxy`). [`DesktopProxies`] holds them by session,
//! never by matter: two sessions of one matter get two proxies.
//!
//! Every OS gets it. Tier C (no jail) leaves the agent's own environment alone: only the browser
//! entry reaches its proxy, through the browser port. The quit stops whatever is still open
//! ([`DesktopProxies::stop_all`]) after the host has stopped, so no proxy thread outlives it.
//!
//! No lock is held while a proxy starts or stops: a start binds two listeners and spawns a
//! thread, and a stop joins it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use clauseo_host::{EgressProxy, HostConfig, HostExit, SessionKey};
use clauseo_sandbox::{ProxyConfig, ProxyHandle};
use tauri::{App, AppHandle, Manager as _, Runtime};

use crate::bridge::lock;
use crate::host_start::HostCell;

/// The open proxies, by session.
#[derive(Debug, Default)]
pub(crate) struct DesktopProxies {
    open: Mutex<BTreeMap<SessionKey, ProxyHandle>>,
}

impl DesktopProxies {
    /// Stops every proxy still open: the quit's, once the host has stopped. Each proxy's thread
    /// has been joined when it returns.
    pub(crate) fn stop_all(&self) {
        let open = std::mem::take(&mut *lock(&self.open));
        // Dropped here, with the lock released: each drop joins its thread.
        drop(open);
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
    let ended = cell.map_or(HostExit::Stopped, crate::host_start::stop);
    if let Some(state) = app.try_state::<EgressState>() {
        state.0.stop_all();
    }
    ended
}

#[cfg(test)]
mod tests;
