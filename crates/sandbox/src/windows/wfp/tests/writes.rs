//! Set-up's writes against WFP in memory (S15 PR7): every object is created with the descriptor
//! owned by Administrators, a provider or sublayer an earlier set-up left with another owner is
//! recreated, and an owner WFP still gets wrong fails the set-up.

use std::cell::RefCell;

use super::super::check::{ERROR_INVALID_OWNER, FoundFilter, OWNER_CHECK, WfpError};
use super::super::spec::{FilterSpec, Namespace, Object};
use super::super::writes::{ADMIN_OWNED, SetupWriter, remove_objects, write_objects};
use super::check::as_written;
use super::{owner, setup};

const ADMINISTRATORS: &str = "S-1-5-32-544";
const SYSTEM: &str = "S-1-5-18";

/// WFP in memory: each object with its owner, every call made.
struct Fake {
    /// What a creation whose descriptor names no owner gets: the token's default owner.
    default_owner: String,
    /// Whether a descriptor's owner is kept (`false`: WFP ignores it).
    honours_owner: bool,
    objects: RefCell<Vec<(Object, String)>>,
    filters: RefCell<Vec<FoundFilter>>,
    calls: RefCell<Vec<String>>,
}

impl Fake {
    /// A fresh WFP where the signed-in user is the default owner, as on CI's runners.
    fn new() -> Fake {
        Fake {
            default_owner: owner().as_str().to_owned(),
            honours_owner: true,
            objects: RefCell::new(Vec::new()),
            filters: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn owner(&self, object: Object) -> Option<String> {
        let objects = self.objects.borrow();
        objects
            .iter()
            .find(|(o, _)| *o == object)
            .map(|(_, owner)| owner.clone())
    }

    fn set(&self, object: Object, owner: &str) {
        let mut objects = self.objects.borrow_mut();
        objects.retain(|(o, _)| *o != object);
        objects.push((object, owner.to_owned()));
    }

    /// Adds `object` unless it is there; the owner `sddl` names, or the default.
    fn create(&self, object: Object, sddl: Option<&str>, call: &str) {
        self.calls.borrow_mut().push(format!("{call} {sddl:?}"));
        if self.owner(object).is_some() {
            return;
        }
        let named = match sddl.and_then(|s| s.strip_prefix("O:")).map(|s| s.get(..2)) {
            Some(Some("BA")) => Some(ADMINISTRATORS),
            Some(Some("SY")) => Some(SYSTEM),
            _ => None,
        };
        let owner = named
            .filter(|_| self.honours_owner)
            .unwrap_or(&self.default_owner);
        self.set(object, owner);
    }

    fn delete(&self, object: Object, call: &str) -> bool {
        self.calls.borrow_mut().push(call.to_owned());
        let mut objects = self.objects.borrow_mut();
        let before = objects.len();
        objects.retain(|(o, _)| *o != object);
        objects.len() != before
    }

    /// The calls made, in order, that start with `prefix`.
    fn calls_of(&self, prefix: &str) -> Vec<String> {
        let calls = self.calls.borrow();
        calls
            .iter()
            .filter(|c| c.starts_with(prefix))
            .cloned()
            .collect()
    }
}

impl SetupWriter for Fake {
    fn add_provider(&self, _: &Namespace, sddl: Option<&str>) -> Result<(), WfpError> {
        self.create(Object::Provider, sddl, "add_provider");
        Ok(())
    }

    fn add_sublayer(&self, _: &Namespace, sddl: Option<&str>) -> Result<(), WfpError> {
        self.create(Object::Sublayer, sddl, "add_sublayer");
        Ok(())
    }

    fn add_filter(
        &self,
        ns: &Namespace,
        spec: &FilterSpec,
        sddl: Option<&str>,
    ) -> Result<u64, WfpError> {
        self.create(Object::Filter(spec.key), sddl, "add_filter");
        let mut filters = self.filters.borrow_mut();
        let id = u64::try_from(filters.len()).unwrap() + 1;
        filters.push(FoundFilter {
            id,
            ..as_written(ns, spec)
        });
        Ok(id)
    }

    fn filters_in(&self, _: &Namespace) -> Result<Vec<FoundFilter>, WfpError> {
        Ok(self.filters.borrow().clone())
    }

    fn delete_by_id(&self, id: u64) -> Result<bool, WfpError> {
        let found = self
            .filters
            .borrow()
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.key);
        self.filters.borrow_mut().retain(|f| f.id != id);
        Ok(found.is_some_and(|key| self.delete(Object::Filter(key), "delete_by_id")))
    }

    fn delete_filter(&self, key: u128) -> Result<bool, WfpError> {
        self.filters.borrow_mut().retain(|f| f.key != key);
        Ok(self.delete(Object::Filter(key), "delete_filter"))
    }

    fn delete_sublayer(&self, _: u128) -> Result<bool, WfpError> {
        Ok(self.delete(Object::Sublayer, "delete_sublayer"))
    }

    fn delete_provider(&self, _: u128) -> Result<bool, WfpError> {
        Ok(self.delete(Object::Provider, "delete_provider"))
    }

    fn owner_of(&self, _: &Namespace, object: Object) -> Result<String, WfpError> {
        self.owner(object).ok_or(WfpError {
            call: "Fwpm*GetSecurityInfoByKey0",
            code: 0x8032_0008,
        })
    }
}

/// Every object `filters` makes in `ns`, provider and sublayer first.
fn objects(filters: &[FilterSpec]) -> Vec<Object> {
    let mut objects = vec![Object::Provider, Object::Sublayer];
    objects.extend(filters.iter().map(|spec| Object::Filter(spec.key)));
    objects
}

#[test]
fn the_creation_descriptor_names_administrators_as_owner_and_leaves_the_dacl_alone() {
    assert_eq!(ADMIN_OWNED, "O:BA");
    assert!(!ADMIN_OWNED.contains("D:") && !ADMIN_OWNED.contains("G:"));
}

#[test]
fn every_object_set_up_creates_is_created_owned_by_administrators() {
    let (ns, wfp) = (Namespace::test(), Fake::new());
    let filters = setup(&[2001, 2002]).filters(&ns);
    assert_eq!(write_objects(&wfp, &ns, &filters), Ok(0));
    let adds = wfp.calls_of("add_");
    assert_eq!(adds.len(), filters.len() + 2, "{adds:?}");
    for add in &adds {
        assert!(add.ends_with("Some(\"O:BA\")"), "{add}");
    }
    for object in objects(&filters) {
        assert_eq!(
            wfp.owner(object).as_deref(),
            Some(ADMINISTRATORS),
            "{object:?}"
        );
    }
}

#[test]
fn a_leftover_owned_by_the_user_is_recreated_owned_by_administrators() {
    let (ns, wfp) = (Namespace::test(), Fake::new());
    let old = setup(&[2001]).filters(&ns);
    // An earlier set-up, before set-up named the owner: everything the user's.
    wfp.add_provider(&ns, None).unwrap();
    wfp.add_sublayer(&ns, None).unwrap();
    for spec in &old {
        wfp.add_filter(&ns, spec, None).unwrap();
    }

    assert_eq!(
        wfp.owner(Object::Provider),
        Some(owner().as_str().to_owned())
    );
    wfp.calls.borrow_mut().clear();

    let filters = setup(&[2001, 2002]).filters(&ns);
    assert_eq!(write_objects(&wfp, &ns, &filters), Ok(0));
    for object in objects(&filters) {
        assert_eq!(
            wfp.owner(object).as_deref(),
            Some(ADMINISTRATORS),
            "{object:?}"
        );
    }
    let calls = wfp.calls.borrow();
    let at = |call: &str| calls.iter().rposition(|c| c.starts_with(call)).unwrap();
    // The old filters go before the sublayer, the sublayer before the provider.
    assert!(at("delete_by_id") < at("delete_sublayer"));
    assert!(at("delete_sublayer") < at("delete_provider"));
    assert!(at("delete_provider") < at("add_provider"));
    assert!(at("add_sublayer") < at("add_filter"));
}

#[test]
fn a_leftover_owned_by_administrators_or_system_is_kept() {
    for kept in [ADMINISTRATORS, SYSTEM] {
        let (ns, wfp) = (Namespace::test(), Fake::new());
        wfp.set(Object::Provider, kept);
        wfp.set(Object::Sublayer, kept);
        let filters = setup(&[2001]).filters(&ns);
        assert_eq!(write_objects(&wfp, &ns, &filters), Ok(0));
        assert!(wfp.calls_of("delete_sublayer").is_empty(), "{kept}");
        assert!(wfp.calls_of("delete_provider").is_empty(), "{kept}");
        assert_eq!(wfp.owner(Object::Provider).as_deref(), Some(kept));
    }
}

#[test]
fn an_owner_wfp_still_gets_wrong_fails_the_set_up() {
    let (ns, mut wfp) = (Namespace::test(), Fake::new());
    wfp.honours_owner = false;
    let filters = setup(&[2001]).filters(&ns);
    assert_eq!(
        write_objects(&wfp, &ns, &filters),
        Err(WfpError {
            call: OWNER_CHECK,
            code: ERROR_INVALID_OWNER,
        })
    );
}

#[test]
fn removal_deletes_every_object() {
    let (ns, wfp) = (Namespace::test(), Fake::new());
    let filters = setup(&[2001, 2002]).filters(&ns);
    write_objects(&wfp, &ns, &filters).unwrap();
    assert_eq!(remove_objects(&wfp, &ns), Ok(0));
    assert!(wfp.objects.borrow().is_empty());
}
