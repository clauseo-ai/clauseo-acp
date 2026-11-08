//! [`ensure`] against a fake registry: the root taken over, subkeys taken over before they are
//! read, links deleted, a planted pinned key recreated empty, the installer's key kept, plants
//! added mid-walk, a deep chain, the pinned forms that aren't changes, and the faults.

use std::collections::BTreeMap;

use super::security::Security;
use super::{INHERIT_SDDL, LINK_SDDL, Report, VENDOR_SDDL, VendorFault, ensure};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::record_values::RECORD_SDDL;

/// A key of a user's, which Users may write subkeys and values into: the weak root.
const WEAK: &str = "O:S-1-5-21-1-2-3-1001D:P(A;CI;KA;;;BU)(A;CI;KA;;;S-1-5-21-1-2-3-1001)";

/// A subkey of a user's, Users may write into it.
const USER_KEY: &str = "O:S-1-5-21-1-2-3-1001D:P(A;CI;KA;;;BU)";

/// The record's pinned SD, as the walk writes it.
const RECORD_PINNED: &str = "O:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)";

/// One fake key: its SDDL, whether it is a link, and its values' names.
#[derive(Clone)]
struct Node {
    sddl: String,
    link: bool,
    values: Vec<String>,
}

/// Keys by path under `HKLM`, with what was done to them, and the faults to inject.
#[derive(Default)]
struct FakeTree {
    keys: BTreeMap<String, Node>,
    /// Each take-over, in order: the path and the SDDL it was given.
    set: Vec<(String, String)>,
    deleted: Vec<String>,
    created: Vec<String>,
    fail_take: Option<String>,
    fail_delete: Option<String>,
    /// Plants made mid-walk: when the take-over of the trigger path happens, the key at the path
    /// is added with its SDDL, and whether it is a link.
    plants: Vec<(String, String, String, bool)>,
}

impl FakeTree {
    /// The root with `root` as its SDDL (none when `None`), and `children` as (path, SDDL, link).
    fn with(root: Option<&str>, children: &[(&str, &str, bool)]) -> FakeTree {
        let mut tree = FakeTree::default();
        if let Some(sddl) = root {
            tree.insert(VENDOR_KEY.to_owned(), sddl, false);
        }
        for (path, sddl, link) in children {
            tree.insert(under(path), sddl, *link);
        }
        tree
    }

    fn insert(&mut self, path: String, sddl: &str, link: bool) {
        self.keys.insert(
            path,
            Node {
                sddl: sddl.to_owned(),
                link,
                values: Vec::new(),
            },
        );
    }

    fn has(&self, path: &str) -> bool {
        self.keys.contains_key(path)
    }

    fn sddl_of(&self, path: &str) -> &str {
        &self.keys[path].sddl
    }

    /// The take-over of `path`: present, with `sddl` set; `None` when it is gone.
    fn take(&mut self, path: &str, sddl: &str) -> Result<Option<String>, String> {
        if !self.has(path) {
            return Ok(None);
        }
        if self.fail_take.as_deref() == Some(path) {
            return Err("RegSetKeySecurity failed (error 5)".to_owned());
        }
        if let Some(node) = self.keys.get_mut(path) {
            node.sddl = sddl.to_owned();
        }
        self.set.push((path.to_owned(), sddl.to_owned()));
        let (due, later): (Vec<_>, Vec<_>) = self
            .plants
            .drain(..)
            .partition(|(trigger, _, _, _)| trigger == path);
        self.plants = later;
        for (_, at, plant, link) in due {
            self.insert(at, &plant, link);
        }
        Ok(Some(path.to_owned()))
    }
}

impl super::Tree for FakeTree {
    type Key = String;

    fn open_root(&mut self) -> Result<Option<String>, String> {
        Ok(self.has(VENDOR_KEY).then(|| VENDOR_KEY.to_owned()))
    }

    fn create_root(&mut self) -> Result<(), String> {
        self.insert(VENDOR_KEY.to_owned(), VENDOR_SDDL, false);
        Ok(())
    }

    fn take_over_root(&mut self, sddl: &str) -> Result<String, String> {
        self.take(VENDOR_KEY, sddl)?
            .ok_or_else(|| "the vendor key is gone".to_owned())
    }

    fn open_child(&mut self, parent: &String, name: &str) -> Result<Option<String>, String> {
        let path = format!("{parent}\\{name}");
        Ok(self.has(&path).then_some(path))
    }

    fn take_over_child(
        &mut self,
        parent: &String,
        name: &str,
        sddl: &str,
    ) -> Result<Option<String>, String> {
        self.take(&format!("{parent}\\{name}"), sddl)
    }

    fn create_child(&mut self, parent: &String, name: &str, sddl: &str) -> Result<String, String> {
        let path = format!("{parent}\\{name}");
        self.insert(path.clone(), sddl, false);
        self.created.push(path.clone());
        Ok(path)
    }

    fn is_link(&mut self, key: &String) -> Result<bool, String> {
        self.keys
            .get(key)
            .map(|node| node.link)
            .ok_or_else(|| format!("{key} is gone"))
    }

    fn sddl(&mut self, key: &String) -> Result<String, String> {
        self.keys
            .get(key)
            .map(|node| node.sddl.clone())
            .ok_or_else(|| format!("{key} is gone"))
    }

    fn set_sddl(&mut self, key: &String, sddl: &str) -> Result<(), String> {
        self.take(key, sddl).map(|_taken| ())
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

/// `path` under the vendor key, as the fake's paths are.
fn under(path: &str) -> String {
    format!("{VENDOR_KEY}\\{path}")
}

/// The record's pinned SDDL, as the walk gives it.
fn record_pinned() -> String {
    format!("O:BA{RECORD_SDDL}")
}

#[test]
fn the_pinned_sddl_is_the_one_the_module_names() {
    assert_eq!(
        VENDOR_SDDL,
        "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)"
    );
    assert_eq!(RECORD_PINNED, record_pinned());
}

#[test]
fn a_missing_root_is_made_and_taken_over() {
    let mut tree = FakeTree::with(None, &[]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report, Report::default());
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert_eq!(tree.created, Vec::<String>::new());
}

#[test]
fn a_weak_root_is_taken_over_and_its_user_subkeys_inherit_and_stay() {
    let mut tree = FakeTree::with(
        Some(WEAK),
        &[
            ("Planted", USER_KEY, false),
            ("Planted\\Nested", USER_KEY, false),
        ],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.weaker_from.as_deref(), Some(WEAK));
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert_eq!(tree.sddl_of(&under("Planted")), INHERIT_SDDL);
    assert_eq!(tree.sddl_of(&under("Planted\\Nested")), INHERIT_SDDL);
    assert_eq!(tree.deleted, Vec::<String>::new(), "only links are deleted");
    assert_eq!(
        report.taken_over,
        [under("Planted"), under("Planted\\Nested")]
    );
}

#[test]
fn a_link_root_is_refused_before_any_write() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", USER_KEY, false)]);
    tree.keys.get_mut(VENDOR_KEY).unwrap().link = true;
    assert_eq!(ensure(&mut tree, VENDOR_KEY), Err(VendorFault::Link));
    assert_eq!(tree.set, Vec::<(String, String)>::new());
    assert_eq!(tree.deleted, Vec::<String>::new());
}

#[test]
fn a_failed_root_take_over_refuses() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", USER_KEY, false)]);
    tree.fail_take = Some(VENDOR_KEY.to_owned());
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::Secure(_))), "{result:?}");
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(
        tree.sddl_of(&under("Planted")),
        USER_KEY,
        "nothing under it changed"
    );
}

#[test]
fn a_failed_subkey_take_over_refuses() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Planted", USER_KEY, false)]);
    tree.fail_take = Some(under("Planted"));
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::Secure(_))), "{result:?}");
}

#[test]
fn a_link_child_is_taken_over_then_deleted() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Link", VENDOR_SDDL, true)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.removed, [under("Link")]);
    assert_eq!(tree.deleted, [under("Link")]);
    assert!(tree.set.contains(&(under("Link"), LINK_SDDL.to_owned())));
}

#[test]
fn a_failed_link_delete_refuses() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Link", VENDOR_SDDL, true)]);
    tree.fail_delete = Some(under("Link"));
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::CleanUp(_))), "{result:?}");
}

#[test]
fn the_installer_key_is_taken_over_and_never_deleted() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Clauseo", USER_KEY, false)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(tree.sddl_of(&under("Clauseo")), INHERIT_SDDL);
    assert_eq!(report.admin_owned, Vec::<String>::new(), "a user owned it");
    let mut admin = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("Clauseo", "O:BAD:P(A;;KA;;;BA)", false)],
    );
    let report = ensure(&mut admin, VENDOR_KEY).unwrap();
    assert_eq!(admin.deleted, Vec::<String>::new());
    assert_eq!(admin.sddl_of(&under("Clauseo")), INHERIT_SDDL);
    assert!(
        report.admin_owned.is_empty(),
        "the installer's name isn't logged"
    );
}

#[test]
fn a_key_that_is_kept_with_a_users_write_ace_inherits() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Kept", "O:SYD:P(A;;KW;;;BU)", false)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.sddl_of(&under("Kept")), INHERIT_SDDL);
    assert_eq!(report.taken_over, [under("Kept")]);
}

#[test]
fn a_planted_sandbox_is_recreated_empty_and_its_values_are_gone() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("Sandbox", USER_KEY, false),
            ("Sandbox\\Inner", USER_KEY, false),
        ],
    );
    tree.keys.get_mut(&under("Sandbox")).unwrap().values = vec!["PoolSids".to_owned()];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
    assert!(
        tree.keys[&under("Sandbox")].values.is_empty(),
        "the seeded value is gone"
    );
    assert!(
        !tree.has(&under("Sandbox\\Inner")),
        "what was under it is gone"
    );
    assert!(tree.deleted.contains(&under("Sandbox")));
    assert_eq!(report.resecured, Vec::<String>::new());
}

#[test]
fn an_admin_owned_sandbox_keeps_its_values_and_gets_its_sd() {
    let drifted = "O:BAD:P(A;CI;KA;;;BU)";
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", drifted, false)]);
    tree.keys.get_mut(&under("Sandbox")).unwrap().values = vec!["PoolSids".to_owned()];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.resecured, [under("Sandbox")]);
    assert!(report.recreated.is_empty() && tree.deleted.is_empty());
    assert_eq!(tree.keys[&under("Sandbox")].values, ["PoolSids"]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
}

#[test]
fn a_pinned_sandbox_and_root_in_another_form_are_not_changes() {
    let sandbox = "O:BAD:PAI(A;CI;KA;;;SY)(A;CI;KA;;;S-1-5-32-544)(A;CI;KR;;;BU)";
    let root =
        "O:S-1-5-18D:PAI(A;OICI;KA;;;S-1-5-18)(A;OICI;KA;;;S-1-5-32-544)(A;OICI;KR;;;S-1-5-32-545)";
    let mut tree = FakeTree::with(Some(root), &[("Sandbox", sandbox, false)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.weaker_from, None);
    assert!(report.resecured.is_empty() && report.taken_over.is_empty());
}

#[test]
fn a_plant_added_mid_walk_ends_taken_over_or_removed() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("A", VENDOR_SDDL, false), ("B", USER_KEY, false)],
    );
    tree.plants = vec![
        (under("A"), under("B\\Plant"), USER_KEY.to_owned(), false),
        (under("A"), under("B\\LinkPlant"), USER_KEY.to_owned(), true),
    ];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.sddl_of(&under("B\\Plant")), INHERIT_SDDL);
    assert!(report.taken_over.contains(&under("B\\Plant")));
    assert!(
        !tree.has(&under("B\\LinkPlant")),
        "the planted link is deleted"
    );
    assert_eq!(report.removed, [under("B\\LinkPlant")]);
}

#[test]
fn a_key_forty_levels_deep_is_taken_over_not_refused() {
    let mut path = String::from("K");
    let mut children = Vec::new();
    for _ in 0..40 {
        children.push((path.clone(), USER_KEY.to_owned(), false));
        path.push_str("\\K");
    }
    let borrowed: Vec<(&str, &str, bool)> = children
        .iter()
        .map(|(path, sddl, link)| (path.as_str(), sddl.as_str(), *link))
        .collect();
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &borrowed);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.taken_over.len(), 40);
    let deepest = vec!["K"; 40].join("\\");
    assert_eq!(tree.sddl_of(&under(&deepest)), INHERIT_SDDL);
}

#[test]
fn an_owner_or_dacl_change_is_a_different_structure() {
    let pinned = Security::parse(VENDOR_SDDL);
    assert_ne!(
        pinned,
        Security::parse("O:BUD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)")
    );
    assert_ne!(
        pinned,
        Security::parse("O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)")
    );
    assert!(pinned.grants("SY", "KA") && pinned.grants("BA", "KA") && pinned.grants("BU", "KR"));
    assert!(!pinned.grants("BU", "KA"));
}
