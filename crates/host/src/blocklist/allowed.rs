//! The sites the lawyer allowed despite the block list (decision record 0053, PR 2), as the
//! egress proxies read them: [`AllowedSites`] hands each launch a live per-matter view, and the
//! actor republishes every view after each allow, revoke and `SettingsChanged` (the policy file
//! is read only at start, so a policy change applies at the next start). What is stored is in
//! `allowed/file.rs`; the commands are the actor's (`actor/allowed.rs`).
//!
//! - A matter's view is its `Matter` allows and every `Always` one, exact hosts; empty while
//!   `network.allow_unsafe_site_override` is off, whatever is stored.
//! - Views are held weakly: one no launch holds any more is dropped at the next publish, so
//!   the map keeps only matters with a running proxy.
//! - The mutex guards in-memory sets only and is never held across I/O; a view's own swap is
//!   `clauseo_blocklist::Allowed`'s.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use clauseo_blocklist::Allowed;
use clauseo_protocol::MatterId;

pub(crate) mod file;

/// The allowed sites in force: per matter and for every matter. Empty when overrides are off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InForce {
    pub(crate) always: BTreeSet<String>,
    pub(crate) matters: BTreeMap<MatterId, BTreeSet<String>>,
}

impl InForce {
    /// What `matter`'s launches may reach although the list names it.
    fn for_matter(&self, matter: MatterId) -> BTreeSet<String> {
        let mut hosts = self.always.clone();
        if let Some(own) = self.matters.get(&matter) {
            hosts.extend(own.iter().cloned());
        }
        hosts
    }
}

/// Every launch's view of the allowed sites, shared between the host (which publishes) and
/// whatever starts the egress proxies (which asks [`AllowedSites::for_matter`]). The desktop
/// makes one, keeps it in Tauri's state and hands it to the host with
/// [`crate::BlockListSeams::with_allowed`]; a host started without one makes its own.
#[derive(Debug, Default)]
pub struct AllowedSites {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    in_force: InForce,
    views: BTreeMap<MatterId, Weak<Allowed>>,
}

impl AllowedSites {
    /// The live view for a launch in `matter`: what `ProxyConfig::allowed` takes. It holds the
    /// matter's allows and the `Always` ones now, and every later change until it is dropped.
    /// Launches of one matter share one view.
    pub fn for_matter(&self, matter: MatterId) -> Arc<Allowed> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(view) = inner.views.get(&matter).and_then(Weak::upgrade) {
            return view;
        }
        let view = Arc::new(Allowed::default());
        view.set(inner.in_force.for_matter(matter));
        inner.views.insert(matter, Arc::downgrade(&view));
        view
    }

    /// Puts `in_force` in place and swaps every live view's set; drops the dead views.
    pub(crate) fn publish(&self, in_force: InForce) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.views.retain(|_, view| view.strong_count() > 0);
        for (matter, view) in &inner.views {
            if let Some(view) = view.upgrade() {
                view.set(in_force.for_matter(*matter));
            }
        }
        inner.in_force = in_force;
    }

    /// How many views are kept (tests).
    #[cfg(test)]
    pub(crate) fn views(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.views.len()
    }
}

#[cfg(test)]
mod tests;
