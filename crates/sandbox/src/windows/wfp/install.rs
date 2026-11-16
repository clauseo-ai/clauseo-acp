//! Set-up's network step and its removal (S15 PR7), run elevated by the setup exe. In one
//! transaction: the provider and the sublayer; every filter already in our sublayer deleted
//! (ours, and any foreign one); the block filters and one static permit per pool user added.
//! So a re-run or an extend replaces the filters with no moment where a pool user is
//! unfiltered. Every object is created owned by Administrators, one an earlier run left owned by
//! another account is recreated, and each owner is read back before the commit (`writes.rs`):
//! one still wrong aborts the transaction and fails the set-up. Then the owner's
//! read-only grants; then the firewall rules, which never fail the set-up
//! ([`FirewallOutcome`]). Any WFP failure, the Base Filtering Engine down included, fails it.

use super::check::{FoundFilter, WfpError};
use super::engine::{Engine, filters_in, grant, owner_of};
use super::firewall;
use super::spec::{
    FilterSpec, FirewallOutcome, Namespace, NetworkReport, NetworkSetup, Object, revocations,
    rule_names,
};
use super::writes::{SetupWriter, remove_objects, write_objects};
use crate::windows::sids::Sid;

impl SetupWriter for Engine {
    fn add_provider(&self, ns: &Namespace, sddl: Option<&str>) -> Result<(), WfpError> {
        Engine::add_provider(self, ns, sddl)
    }

    fn add_sublayer(&self, ns: &Namespace, sddl: Option<&str>) -> Result<(), WfpError> {
        Engine::add_sublayer(self, ns, sddl)
    }

    fn add_filter(
        &self,
        ns: &Namespace,
        spec: &FilterSpec,
        sddl: Option<&str>,
    ) -> Result<u64, WfpError> {
        Engine::add_filter(self, ns, spec, sddl)
    }

    fn filters_in(&self, ns: &Namespace) -> Result<Vec<FoundFilter>, WfpError> {
        filters_in(self, ns).map_err(|(call, code)| WfpError { call, code })
    }

    fn delete_by_id(&self, id: u64) -> Result<bool, WfpError> {
        Engine::delete_by_id(self, id)
    }

    fn delete_filter(&self, key: u128) -> Result<bool, WfpError> {
        Engine::delete_filter(self, key)
    }

    fn delete_sublayer(&self, key: u128) -> Result<bool, WfpError> {
        Engine::delete_sublayer(self, key)
    }

    fn delete_provider(&self, key: u128) -> Result<bool, WfpError> {
        Engine::delete_provider(self, key)
    }

    fn owner_of(&self, ns: &Namespace, object: Object) -> Result<String, WfpError> {
        owner_of(self, ns, object)
    }
}

/// Writes set-up's network objects in `ns` for `setup`. Idempotent: the sublayer is emptied and
/// refilled for the whole pool in one transaction, a grant already there changes nothing, and
/// each firewall rule is rewritten in place. A new pool user is covered by the next run.
pub fn install(ns: &Namespace, setup: &NetworkSetup) -> Result<NetworkReport, WfpError> {
    let engine = Engine::open(false, &ns.display)?;
    let filters = setup.filters(ns);
    let transaction = engine.transaction()?;
    // Owners are read back inside the transaction: a wrong one aborts it, so nothing another
    // account owns is left committed.
    let foreign_removed = write_objects(&engine, ns, &filters)?;
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
    let foreign_removed = remove_objects(&engine, ns)?;
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
