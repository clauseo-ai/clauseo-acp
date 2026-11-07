//! Set-up's network step and its removal (S15 PR7), run elevated by the setup exe: the provider,
//! the sublayer and the block filters in one transaction, so a re-run or an extend replaces the
//! filters with no moment where a pool user is unfiltered; then the owner's grants; then the
//! firewall rules, which never fail the set-up ([`FirewallOutcome`]).

use super::check::WfpError;
use super::engine::{Engine, grant};
use super::firewall;
use super::spec::{
    FirewallOutcome, Namespace, NetworkReport, NetworkSetup, grants, revocations, rule_names,
};
use crate::windows::sids::Sid;

/// Writes set-up's network objects in `ns` for `setup`. Idempotent: each block filter is
/// deleted by key and added again naming the whole pool, every key removal knows is deleted
/// first (so no filter of an older layout lingers), a grant already there changes nothing, and
/// each firewall rule is rewritten in place. A new pool user is covered by the next run.
pub fn install(ns: &Namespace, setup: &NetworkSetup) -> Result<NetworkReport, WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let filters = setup.filters(ns);
    let transaction = engine.transaction()?;
    engine.add_provider(ns)?;
    engine.add_sublayer(ns)?;
    for key in ns.removal_keys() {
        engine.delete_filter(key)?;
    }
    for spec in &filters {
        engine.add_filter(ns, spec)?;
    }
    transaction.commit()?;
    for each in grants(ns) {
        grant(&engine, ns, each.object, setup.owner(), each.mask, false)?;
    }
    let firewall = firewall::ensure(&setup.rules(ns));
    if let FirewallOutcome::Ineffective(why) = &firewall {
        tracing::warn!(%why, "the firewall rules don't all take effect; WFP's filters still do");
    }
    Ok(NetworkReport {
        filters: filters.len(),
        covered: setup.pool().to_vec(),
        firewall,
    })
}

/// What [`remove`] left of the firewall rules: `Err` with why when one couldn't be removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRemoved {
    /// The firewall rules' removal.
    pub firewall: Result<(), String>,
}

/// Removes set-up's network objects in `ns`, elevated (S15 `PR4j`'s `--remove` calls it): every
/// block filter key, the sublayer and the provider in one transaction, then `owner`'s grants on
/// the filter container and the permit's layer (the others went with their objects), then the
/// firewall rules. Anything already gone counts as removed, so it can run again after a partial
/// removal. The sublayer can't go while a launch's permit is in it: removal runs with every
/// session closed. A WFP failure is the `Err`; a firewall one is in [`NetworkRemoved`].
pub fn remove(ns: &Namespace, owner: Option<&Sid>) -> Result<NetworkRemoved, WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let transaction = engine.transaction()?;
    for key in ns.removal_keys() {
        engine.delete_filter(key)?;
    }
    engine.delete_sublayer(ns.sublayer)?;
    engine.delete_provider(ns.provider)?;
    transaction.commit()?;
    if let Some(owner) = owner {
        for object in revocations() {
            grant(&engine, ns, object, owner, 0, true)?;
        }
    }
    Ok(NetworkRemoved {
        firewall: firewall::remove(&rule_names(ns)),
    })
}
