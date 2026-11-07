//! Set-up's network step and its removal (S15 PR7), run elevated by the setup exe. In one
//! transaction: the provider and the sublayer; every filter already in our sublayer deleted
//! (ours, and any foreign one); the block filters and one static permit per pool user added.
//! So a re-run or an extend replaces the filters with no moment where a pool user is
//! unfiltered. Then the owner's read-only grants; then the firewall rules, which never fail the
//! set-up ([`FirewallOutcome`]). Any WFP failure, the Base Filtering Engine down included,
//! fails it.

use super::check::{Kind, WfpError, classify};
use super::engine::{Engine, filters_in, grant};
use super::firewall;
use super::spec::{
    FirewallOutcome, Namespace, NetworkReport, NetworkSetup, revocations, rule_names,
};
use crate::windows::sids::Sid;

/// Deletes every filter in `ns`'s sublayer and every key set-up has ever written; how many of
/// those deleted weren't set-up's own.
fn clear_sublayer(engine: &Engine, ns: &Namespace) -> Result<usize, WfpError> {
    let found = filters_in(engine, ns).map_err(|(call, code)| WfpError { call, code })?;
    let mut foreign = 0;
    for filter in &found {
        if classify(ns, filter) == Kind::Foreign {
            foreign += 1;
            tracing::warn!(name = %filter.name, "set-up deletes a filter it didn't write");
        }
        engine.delete_by_id(filter.id)?;
    }
    for key in ns.removal_keys() {
        engine.delete_filter(key)?;
    }
    Ok(foreign)
}

/// Writes set-up's network objects in `ns` for `setup`. Idempotent: the sublayer is emptied and
/// refilled for the whole pool in one transaction, a grant already there changes nothing, and
/// each firewall rule is rewritten in place. A new pool user is covered by the next run.
pub fn install(ns: &Namespace, setup: &NetworkSetup) -> Result<NetworkReport, WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let filters = setup.filters(ns);
    let transaction = engine.transaction()?;
    engine.add_provider(ns)?;
    engine.add_sublayer(ns)?;
    let foreign_removed = clear_sublayer(&engine, ns)?;
    for spec in &filters {
        engine.add_filter(ns, spec)?;
    }
    transaction.commit()?;
    for each in setup.grants(ns) {
        grant(&engine, ns, each.object, setup.owner(), each.mask, false)?;
    }
    let firewall = firewall::ensure(&setup.rules(ns));
    if let FirewallOutcome::Ineffective(why) = &firewall {
        tracing::warn!(%why, "the firewall rules don't all take effect; WFP's filters still do");
    }
    Ok(NetworkReport {
        filters: filters.len(),
        foreign_removed,
        covered: setup.pool().to_vec(),
        firewall,
    })
}

/// What [`remove`] left of the firewall rules: `Err` with why when one couldn't be removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRemoved {
    /// How many filters in our sublayer it deleted that weren't set-up's.
    pub foreign_removed: usize,
    /// The firewall rules' removal.
    pub firewall: Result<(), String>,
}

/// Removes set-up's network objects in `ns`, elevated (S15 `PR4j`'s `--remove` calls it): every
/// filter in our sublayer (foreign ones too, so the sublayer can go) and every key set-up has
/// written, the sublayer and the provider in one transaction; then `owner`'s read grant on the
/// filter container (the others went with their objects); then the firewall rules. Anything
/// already gone counts as removed, so it can run again after a partial removal. A WFP failure
/// is the `Err`; a firewall one is in [`NetworkRemoved`].
pub fn remove(ns: &Namespace, owner: Option<&Sid>) -> Result<NetworkRemoved, WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let transaction = engine.transaction()?;
    let foreign_removed = clear_sublayer(&engine, ns)?;
    engine.delete_sublayer(ns.sublayer)?;
    engine.delete_provider(ns.provider)?;
    transaction.commit()?;
    if let Some(owner) = owner {
        for object in revocations() {
            grant(&engine, ns, object, owner, 0, true)?;
        }
    }
    Ok(NetworkRemoved {
        foreign_removed,
        firewall: firewall::remove(&rule_names(ns)),
    })
}
