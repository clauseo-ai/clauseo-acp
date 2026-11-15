//! What the handle and the proxy thread share: the live list, the mode, and the open tunnels.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use futures::future::{AbortHandle, AbortRegistration};

use crate::proxy::allowlist::Name;
use crate::proxy::{Allowlist, Listener, ProxyMode};

/// The live list and the open tunnels, under one lock so a `revoke` cannot miss a tunnel being
/// opened, and the launch's mode, which never changes. Open mode opens the browser listener
/// only: the agent listener asks the list in both modes.
pub(crate) struct Shared {
    mode: ProxyMode,
    live: Mutex<Live>,
}

pub(crate) struct Live {
    pub(crate) allow: Allowlist,
    /// Each open tunnel's host, whether it was let in without the list (open mode's browser
    /// listener), and its abort handle.
    tunnels: BTreeMap<u64, (Name, bool, AbortHandle)>,
    next: u64,
}

/// Why a tunnel was not opened.
pub(crate) enum Unopened {
    /// The list no longer allows the host (a `revoke` came during the connect).
    NotAllowed,
    /// The launch has its most tunnels open.
    Busy,
    /// The lawyer's allow for the host was revoked since the block list was asked, and the
    /// list still names it.
    Blocked,
}

impl Shared {
    pub(crate) fn new(allow: Allowlist, mode: ProxyMode) -> Shared {
        Shared {
            mode,
            live: Mutex::new(Live {
                allow,
                tunnels: BTreeMap::new(),
                next: 0,
            }),
        }
    }

    pub(crate) fn live(&self) -> MutexGuard<'_, Live> {
        // The list and the tunnel table stay valid whatever a panicking thread left.
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a request on `which` skips the allowlist: open mode's browser listener only.
    pub(crate) fn listless(&self, which: Listener) -> bool {
        self.mode == ProxyMode::Open && which == Listener::Browser
    }

    /// Whether the allowlist check passes `name` on `which`: on the live list, or any host
    /// when the listener skips the list.
    pub(crate) fn admits(&self, name: &Name, which: Listener) -> bool {
        self.listless(which) || self.live().allow.contains_name(name)
    }

    /// Opens a tunnel to `name` from `which` if the list still allows it (any host when the
    /// listener skips the list), `still` (asked under the lock, so a revoke's watcher cannot
    /// miss the tunnel; it must do no I/O) says the block list still lets it through, and there
    /// is room.
    pub(crate) fn open(
        &self,
        name: &Name,
        which: Listener,
        most: usize,
        still: impl FnOnce(&Name) -> bool,
    ) -> Result<(u64, AbortRegistration), Unopened> {
        let listless = self.listless(which);
        let mut live = self.live();
        if !listless && !live.allow.contains_name(name) {
            return Err(Unopened::NotAllowed);
        }
        if !still(name) {
            return Err(Unopened::Blocked);
        }
        if live.tunnels.len() >= most {
            return Err(Unopened::Busy);
        }
        let (handle, registration) = AbortHandle::new_pair();
        let id = live.next;
        live.next += 1;
        live.tunnels.insert(id, (name.clone(), listless, handle));
        Ok((id, registration))
    }

    pub(crate) fn close(&self, id: u64) {
        self.live().tunnels.remove(&id);
    }

    /// Closes every open tunnel whose host `close` names.
    pub(crate) fn close_where(&self, close: impl Fn(&Name) -> bool) {
        self.live().tunnels.retain(|_, (host, _, handle)| {
            let shut = close(host);
            if shut {
                handle.abort();
            }
            !shut
        });
    }

    /// Removes `name` from the live list and closes the tunnels whose host it no longer allows,
    /// except those let in without the list (open mode's browser listener), which it never
    /// decided.
    pub(crate) fn revoke(&self, name: &Name) {
        let mut live = self.live();
        live.allow.remove_exact(name);
        let Live { allow, tunnels, .. } = &mut *live;
        tunnels.retain(|_, (host, listless, handle)| {
            let keep = *listless || allow.contains_name(host);
            if !keep {
                handle.abort();
            }
            keep
        });
    }
}
