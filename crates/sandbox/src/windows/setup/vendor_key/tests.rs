//! [`ensure`] against a fake registry: the pinned SD, a weak parent re-secured with its planted
//! subkey removed, a link refused, a drifted Clauseo key set back, the rest left alone, and a
//! fault refusing.

use std::collections::BTreeMap;

use super::{RECORD_NAME, VENDOR_SDDL, VendorFault, ensure};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::record_values::RECORD_SDDL;

/// The vendor key's record subkey's pinned security, as the module names it.
const RECORD_PINNED: &str = "O:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)";

/// A user-owned key with a DACL that lets Users make subkeys.
const WEAK: &str = "O:S-1-5-21-1-2-3-1001D:P(A;CI;KA;;;BU)(A;CI;KA;;;S-1-5-21-1-2-3-1001)";

/// One fake key: its SDDL and whether it is a link.
#[derive(Clone)]
struct Node {
    sddl: String,
    link: bool,
}

/// Keys by their path under `HKLM`, with the faults to inject and what was done to them.
#[derive(Default)]
struct FakeTree {
    keys: BTreeMap<String, Node>,
    deleted: Vec<String>,
    set: Vec<(String, String)>,
    fail_set: Option<String>,
    fail_delete: Option<String>,
}

impl FakeTree {
    /// The vendor key with `sddl` (none when `None`), and `children` as (path, SDDL, link).
    fn with(root: Option<&str>, children: &[(&str, &str, bool)]) -> FakeTree {
        let mut tree = FakeTree::default();
        if let Some(sddl) = root {
            tree.keys.insert(VENDOR_KEY.to_owned(), node(sddl, false));
        }
        for (path, sddl, link) in children {
            tree.keys
                .insert(format!("{VENDOR_KEY}\\{path}"), node(sddl, *link));
        }
        tree
    }

    fn has(&self, path: &str) -> bool {
        self.keys.contains_key(path)
    }

    fn sddl_of(&self, path: &str) -> &str {
        &self.keys[path].sddl
    }
}

fn node(sddl: &str, link: bool) -> Node {
    Node {
        sddl: sddl.to_owned(),
        link,
    }
}

impl super::Tree for FakeTree {
    type Key = String;

    fn open_root(&mut self) -> Result<Option<String>, String> {
        Ok(self.has(VENDOR_KEY).then(|| VENDOR_KEY.to_owned()))
    }

    fn create_root(&mut self) -> Result<(), String> {
        self.keys
            .insert(VENDOR_KEY.to_owned(), node(VENDOR_SDDL, false));
        Ok(())
    }

    fn is_link(&mut self, key: &String) -> Result<bool, String> {
        Ok(self.keys[key].link)
    }

    fn sddl(&mut self, key: &String) -> Result<String, String> {
        Ok(self.keys[key].sddl.clone())
    }

    fn set_sddl(&mut self, key: &String, sddl: &str) -> Result<(), String> {
        if self.fail_set.as_deref() == Some(key.as_str()) {
            return Err("RegSetKeySecurity failed (error 5)".to_owned());
        }
        self.set.push((key.clone(), sddl.to_owned()));
        self.keys.get_mut(key).ok_or("gone")?.sddl = sddl.to_owned();
        Ok(())
    }

    fn subkeys(&mut self, key: &String) -> Result<Vec<String>, String> {
        let prefix = format!("{key}\\");
        Ok(self
            .keys
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('\\'))
            .map(str::to_owned)
            .collect())
    }

    fn open_child(&mut self, parent: &String, name: &str) -> Result<String, String> {
        let path = format!("{parent}\\{name}");
        if self.has(&path) {
            Ok(path)
        } else {
            Err(format!("{path} is gone"))
        }
    }

    fn delete(&mut self, key: String) -> Result<(), String> {
        if self.fail_delete.as_deref() == Some(key.as_str()) {
            return Err("NtDeleteKey failed (0xc0000022)".to_owned());
        }
        if !self.subkeys(&key)?.is_empty() {
            return Err("NtDeleteKey failed: the key has subkeys".to_owned());
        }
        self.keys.remove(&key);
        self.deleted.push(key);
        Ok(())
    }
}

/// `path` under the vendor key, for the fake's paths.
fn under(path: &str) -> String {
    format!("{VENDOR_KEY}\\{path}")
}

#[test]
fn the_pinned_sddl_owns_by_system_and_keeps_others_out() {
    assert_eq!(
        VENDOR_SDDL,
        "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)"
    );
    assert_eq!(RECORD_PINNED, format!("O:BA{RECORD_SDDL}"));
}

#[test]
fn a_missing_vendor_key_is_made_with_the_pinned_sddl() {
    let mut tree = FakeTree::with(None, &[]);
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report, super::Report::default(), "nothing to log");
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
}

#[test]
fn a_weak_vendor_key_is_resecured_and_its_planted_subkey_removed() {
    let planted = under("Planted");
    let nested = under("Planted\\Nested");
    let mut tree = FakeTree::with(
        Some(WEAK),
        &[("Planted", WEAK, false), ("Planted\\Nested", WEAK, false)],
    );
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report.weaker_from.as_deref(), Some(WEAK));
    assert_eq!(report.removed, [under("Planted")]);
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert!(!tree.has(&planted) && !tree.has(&nested));
    assert_eq!(tree.deleted, [nested, planted], "children first, by handle");
}

#[test]
fn a_linked_vendor_key_is_refused_before_anything_is_written() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", WEAK, false)]);
    tree.keys.get_mut(VENDOR_KEY).unwrap().link = true;
    assert_eq!(ensure(&mut tree), Err(VendorFault::Link));
    assert!(tree.set.is_empty() && tree.deleted.is_empty());
}

#[test]
fn a_failed_resecure_refuses_and_removes_nothing() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", WEAK, false)]);
    tree.fail_set = Some(VENDOR_KEY.to_owned());
    let result = ensure(&mut tree);
    assert!(matches!(result, Err(VendorFault::Secure(_))), "{result:?}");
    assert_eq!(tree.deleted, Vec::<String>::new());
}

#[test]
fn a_failed_clean_up_refuses() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Planted", WEAK, false)]);
    tree.fail_delete = Some(under("Planted"));
    let result = ensure(&mut tree);
    assert!(matches!(result, Err(VendorFault::CleanUp(_))), "{result:?}");
}

#[test]
fn a_linked_subkey_is_deleted_whoever_owns_it() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Link", VENDOR_SDDL, true)]);
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report.removed, [under("Link")]);
    assert!(!tree.has(&under("Link")));
}

#[test]
fn a_drifted_record_key_is_set_back_to_its_pinned_sddl() {
    let drifted = "O:BAD:P(A;CI;KA;;;BU)";
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[(RECORD_NAME, drifted, false)]);
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report.resecured, [under(RECORD_NAME)]);
    assert_eq!(tree.sddl_of(&under(RECORD_NAME)), RECORD_PINNED);
    assert_eq!(report.removed, Vec::<String>::new());
}

#[test]
fn a_record_key_owned_by_someone_else_is_removed() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[(RECORD_NAME, WEAK, false)]);
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report.removed, [under(RECORD_NAME)]);
    assert!(!tree.has(&under(RECORD_NAME)));
}

#[test]
fn the_keys_that_are_right_are_left_alone() {
    let other = "O:BAD:P(A;;KA;;;BA)";
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            (RECORD_NAME, RECORD_PINNED, false),
            ("Clauseo", other, false),
            ("Clauseo\\Child", "O:SYD:P(A;;KA;;;SY)", false),
        ],
    );
    let report = ensure(&mut tree).unwrap();
    assert_eq!(report, super::Report::default());
    assert!(
        tree.set.iter().all(|(key, _)| key == VENDOR_KEY),
        "{:?}",
        tree.set
    );
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(tree.sddl_of(&under("Clauseo")), other);
}

#[test]
fn a_subkey_nested_past_the_limit_refuses() {
    let mut path = String::from("K");
    let mut children = Vec::new();
    for _ in 0..40 {
        children.push((path.clone(), VENDOR_SDDL.to_owned(), false));
        path.push_str("\\K");
    }
    let borrowed: Vec<(&str, &str, bool)> = children
        .iter()
        .map(|(path, sddl, link)| (path.as_str(), sddl.as_str(), *link))
        .collect();
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &borrowed);
    assert_eq!(ensure(&mut tree), Err(VendorFault::TooDeep));
}
