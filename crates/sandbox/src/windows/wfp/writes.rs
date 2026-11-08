//! What set-up and its removal write to WFP inside their transaction (S15 PR7), over
//! [`SetupWriter`] so it is tested on every OS; `install.rs` runs it on the real engine.
//!
//! WFP gives an object the creating token's default owner, and on an administrator's elevated
//! token that may be the signed-in user (the "System objects: Default owner for objects created
//! by members of the Administrators group" policy set to "Object creator", as on CI's runners).
//! An owner may rewrite the object's DACL, so set-up creates every object with [`ADMIN_OWNED`]:
//! owner Administrators, no DACL, so WFP's default DACL stays as it was. A provider or sublayer
//! an earlier set-up left with another owner is deleted and created again in the same
//! transaction (`Fwpm*SetSecurityInfoByKey0` can't run inside one), and the filters are
//! recreated on every run anyway. Every owner is then read back, strictly
//! ([`owner_allowed`]): one still wrong aborts the transaction.

use super::check::{
    ERROR_INVALID_OWNER, FoundFilter, Kind, OWNER_CHECK, WfpError, classify, owner_allowed,
};
use super::spec::{FilterSpec, Namespace, Object};

/// The descriptor every object set-up creates is created with, in SDDL: owner `BUILTIN\
/// Administrators` (`BA`), no group and no DACL, so WFP still applies its default DACL.
pub(super) const ADMIN_OWNED: &str = "O:BA";

/// The WFP calls set-up's writes make. A creation names the descriptor it passes (`None`: WFP's
/// default, for the owner probe alone).
pub(super) trait SetupWriter {
    /// Begins an explicit transaction.
    fn begin(&self) -> Result<(), WfpError>;
    /// Commits it.
    fn commit(&self) -> Result<(), WfpError>;
    /// Aborts it: nothing written since [`SetupWriter::begin`] takes effect.
    fn abort(&self);
    /// Adds the persistent provider; one already there is kept.
    fn add_provider(&self, ns: &Namespace, sddl: Option<&str>) -> Result<(), WfpError>;
    /// Adds the persistent sublayer under the provider; one already there is kept.
    fn add_sublayer(&self, ns: &Namespace, sddl: Option<&str>) -> Result<(), WfpError>;
    /// Adds `spec` in the sublayer; its WFP id.
    fn add_filter(
        &self,
        ns: &Namespace,
        spec: &FilterSpec,
        sddl: Option<&str>,
    ) -> Result<u64, WfpError>;
    /// Every filter in the sublayer.
    fn filters_in(&self, ns: &Namespace) -> Result<Vec<FoundFilter>, WfpError>;
    /// Deletes the filter with WFP id `id`; `false` when it wasn't there.
    fn delete_by_id(&self, id: u64) -> Result<bool, WfpError>;
    /// Deletes the filter `key`; `false` when it wasn't there.
    fn delete_filter(&self, key: u128) -> Result<bool, WfpError>;
    /// Deletes the sublayer `key`; `false` when it wasn't there.
    fn delete_sublayer(&self, key: u128) -> Result<bool, WfpError>;
    /// Deletes the provider `key`; `false` when it wasn't there.
    fn delete_provider(&self, key: u128) -> Result<bool, WfpError>;
    /// The owner of `object`, as a SID string.
    fn owner_of(&self, ns: &Namespace, object: Object) -> Result<String, WfpError>;
}

/// Deletes every filter in `ns`'s sublayer and every key set-up has ever written; how many of
/// those deleted weren't set-up's own.
pub(super) fn clear_sublayer(wfp: &impl SetupWriter, ns: &Namespace) -> Result<usize, WfpError> {
    let mut foreign = 0;
    for filter in &wfp.filters_in(ns)? {
        if classify(ns, filter) == Kind::Foreign {
            foreign += 1;
            tracing::warn!(name = %filter.name, "set-up deletes a filter it didn't write");
        }
        wfp.delete_by_id(filter.id)?;
    }
    for key in ns.removal_keys() {
        wfp.delete_filter(key)?;
    }
    Ok(foreign)
}

/// Whether `object`'s owner is one set-up accepts.
fn owned_well(wfp: &impl SetupWriter, ns: &Namespace, object: Object) -> Result<bool, WfpError> {
    Ok(owner_allowed(&wfp.owner_of(ns, object)?))
}

/// Writes `filters` in `ns`, inside the caller's transaction: the provider and the sublayer
/// (each recreated when an earlier set-up left it with another owner), the sublayer emptied,
/// the filters added, all with [`ADMIN_OWNED`], then every owner read back. How many filters
/// it deleted that weren't set-up's; a WFP failure, or an owner still wrong, is the `Err`.
pub(super) fn write_objects(
    wfp: &impl SetupWriter,
    ns: &Namespace,
    filters: &[FilterSpec],
) -> Result<usize, WfpError> {
    wfp.add_provider(ns, Some(ADMIN_OWNED))?;
    wfp.add_sublayer(ns, Some(ADMIN_OWNED))?;
    let foreign = clear_sublayer(wfp, ns)?;
    if !owned_well(wfp, ns, Object::Provider)? || !owned_well(wfp, ns, Object::Sublayer)? {
        tracing::warn!("set-up recreates its WFP provider and sublayer, owned by another account");
        wfp.delete_sublayer(ns.sublayer)?;
        wfp.delete_provider(ns.provider)?;
        wfp.add_provider(ns, Some(ADMIN_OWNED))?;
        wfp.add_sublayer(ns, Some(ADMIN_OWNED))?;
    }
    for spec in filters {
        wfp.add_filter(ns, spec, Some(ADMIN_OWNED))?;
    }
    let mut objects = vec![Object::Provider, Object::Sublayer];
    objects.extend(filters.iter().map(|spec| Object::Filter(spec.key)));
    for object in objects {
        if !owned_well(wfp, ns, object)? {
            tracing::warn!(
                ?object,
                "one of set-up's WFP objects isn't owned by Administrators"
            );
            return Err(WfpError {
                call: OWNER_CHECK,
                code: ERROR_INVALID_OWNER,
            });
        }
    }
    Ok(foreign)
}

/// Replaces every filter in `ns`'s sublayer with `filters` ([`write_objects`]) in one WFP
/// transaction, so no moment has both an earlier set-up's filters and these, or neither: an
/// earlier set-up's shared loopback permits go in the same commit that adds the per-slot
/// filters. A failure aborts it, leaving what was there.
pub(super) fn replace(
    wfp: &impl SetupWriter,
    ns: &Namespace,
    filters: &[FilterSpec],
) -> Result<usize, WfpError> {
    wfp.begin()?;
    let written = write_objects(wfp, ns, filters).and_then(|foreign| {
        wfp.commit()?;
        Ok(foreign)
    });
    if written.is_err() {
        wfp.abort();
    }
    written
}

/// Deletes set-up's objects in `ns`, inside the caller's transaction: every filter in the
/// sublayer, every key set-up has written, the sublayer and the provider. How many filters it
/// deleted that weren't set-up's.
pub(super) fn remove_objects(wfp: &impl SetupWriter, ns: &Namespace) -> Result<usize, WfpError> {
    let foreign = clear_sublayer(wfp, ns)?;
    wfp.delete_sublayer(ns.sublayer)?;
    wfp.delete_provider(ns.provider)?;
    Ok(foreign)
}
