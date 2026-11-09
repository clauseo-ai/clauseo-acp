//! [`ensure`] against a fake registry: the root taken over first and its link value judged,
//! subkeys taken over before they are read, links and plants deleted with their subtrees, keys
//! owned by an administrator's account kept, unresolved owners, the pinned `Sandbox` made again
//! whenever it ends deleted, keys deleted under the walk, plants added mid-walk, a deep chain,
//! and the faults.

use std::collections::BTreeMap;

use super::{INHERITED_SDDL, LOCKED_SDDL, Miss, Report, VENDOR_SDDL, VendorFault, ensure};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::record_values::RECORD_SDDL;

mod access;
mod installer;
mod owners;
mod verify;

/// A key of a user's, which Users may write subkeys and values into: the weak root.
const WEAK: &str = "O:S-1-5-21-1-2-3-1001D:P(A;CI;KA;;;BU)(A;CI;KA;;;S-1-5-21-1-2-3-1001)";

/// A subkey a user (not an administrator) owns; Users may write into it.
const USER_KEY: &str = "O:S-1-5-21-1-2-3-1001D:P(A;CI;KA;;;BU)";

/// A subkey Administrators own, which only they may write.
const ADMIN_KEY: &str = "O:BAD:P(A;;KA;;;BA)";

/// A key owned by an account in Administrators (UAC off, or the built-in Administrator).
const ADMIN_USER_KEY: &str = "O:S-1-5-21-1-2-3-500D:P(A;;KA;;;BA)";

/// A key whose owner's membership can't be resolved (a deleted account).
const GHOST_KEY: &str = "O:S-1-5-21-1-2-3-1999D:P(A;;KA;;;BA)";

/// One fake key: its SDDL, whether it is a real link, whether it holds a link value, and its
/// values' names.
#[derive(Clone)]
struct Node {
    sddl: String,
    link: bool,
    link_value: bool,
    values: Vec<String>,
}

/// What happens when a take-over of a trigger path is done, as a planter with an open handle,
/// or a writer racing the walk, might do.
#[derive(Clone)]
enum Event {
    /// The key at the path is added with its SDDL; `true` for a link.
    Plant(String, String, bool),
    /// The key at the path and its subtree are deleted.
    Vanish(String),
    /// A `SymbolicLinkValue` is written to the key at the path.
    LinkValue(String),
    /// The key at the path gets the SDDL, as a handle held from before its take-over writes it.
    Undo(String, String),
}

/// Keys by path under `HKLM`, with what was done to them, and the faults to inject.
#[derive(Default)]
struct FakeTree {
    keys: BTreeMap<String, Node>,
    /// Each write, in order: the path and the SDDL it was given.
    set: Vec<(String, String)>,
    deleted: Vec<String>,
    created: Vec<String>,
    fail_take: Option<String>,
    fail_delete: Option<String>,
    /// Events by the path whose take-over sets them off, each once.
    events: Vec<(String, Event)>,
    /// Membership of Administrators by owner SID; any other is not a member.
    admins: BTreeMap<String, Result<bool, String>>,
    /// The uninstall key's `InstallLocation`.
    install_location: Option<String>,
    /// The keys whose install folder was written again or deleted.
    install_resets: Vec<String>,
    /// A key that gets the SDDL again right after it is taken over again, and the SDDL.
    retake_undone: Option<(String, String)>,
}

impl FakeTree {
    /// The root with `root` as its SDDL (none when `None`), and `children` as (path, SDDL, link).
    fn with(root: Option<&str>, children: &[(&str, &str, bool)]) -> FakeTree {
        let mut tree = FakeTree::default();
        tree.admins
            .insert("S-1-5-21-1-2-3-500".to_owned(), Ok(true));
        tree.admins.insert(
            "S-1-5-21-1-2-3-1999".to_owned(),
            Err("the account has no name".to_owned()),
        );
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
                link_value: link,
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

    fn node(&mut self, path: &str) -> Result<&mut Node, Miss> {
        self.keys.get_mut(path).ok_or(Miss::Gone)
    }

    fn values(&mut self, path: &str, values: &[&str]) {
        self.keys.get_mut(path).unwrap().values = values.iter().map(|v| (*v).to_owned()).collect();
    }

    /// The take-over of `path` with `sddl`, then the events it sets off; the path and its SDDL
    /// from before.
    fn take(&mut self, path: &str, sddl: &str) -> Result<(String, String), Miss> {
        if self.fail_take.as_deref() == Some(path) {
            return Err(Miss::Failed(
                "RegSetKeySecurity failed (error 5)".to_owned(),
            ));
        }
        let before = std::mem::replace(&mut self.node(path)?.sddl, sddl.to_owned());
        self.set.push((path.to_owned(), sddl.to_owned()));
        let (due, later): (Vec<_>, Vec<_>) = self
            .events
            .drain(..)
            .partition(|(trigger, _)| trigger == path);
        self.events = later;
        for (_, event) in due {
            match event {
                Event::Plant(at, sddl, link) => self.insert(at, &sddl, link),
                Event::Vanish(at) => {
                    let below = format!("{at}\\");
                    self.keys
                        .retain(|key, _| key != &at && !key.starts_with(&below));
                }
                Event::LinkValue(at) => self.node(&at)?.link_value = true,
                Event::Undo(at, sddl) => self.node(&at)?.sddl = sddl,
            }
        }
        Ok((path.to_owned(), before))
    }
}

impl super::Tree for FakeTree {
    type Key = String;

    fn open_root(&mut self) -> Result<Option<String>, Miss> {
        Ok(self.has(VENDOR_KEY).then(|| VENDOR_KEY.to_owned()))
    }

    fn create_root(&mut self) -> Result<(), Miss> {
        self.insert(VENDOR_KEY.to_owned(), VENDOR_SDDL, false);
        self.created.push(VENDOR_KEY.to_owned());
        Ok(())
    }

    fn take_over_root(&mut self, sddl: &str) -> Result<String, Miss> {
        self.take(VENDOR_KEY, sddl).map(|(key, _)| key)
    }

    fn root_leads_elsewhere(&mut self, key: &String) -> Result<bool, Miss> {
        Ok(self.node(key)?.link)
    }

    fn take_over_child(
        &mut self,
        parent: &String,
        name: &str,
        sddl: &str,
    ) -> Result<(String, String), Miss> {
        self.node(parent)?;
        self.take(&format!("{parent}\\{name}"), sddl)
    }

    fn create_child(&mut self, parent: &String, name: &str, sddl: &str) -> Result<String, Miss> {
        self.node(parent)?;
        let path = format!("{parent}\\{name}");
        self.insert(path.clone(), sddl, false);
        self.created.push(path.clone());
        Ok(path)
    }

    fn retake(&mut self, key: &String, sddl: &str) -> Result<(), Miss> {
        self.set_sddl(key, sddl)?;
        if let Some((path, undone)) = self.retake_undone.clone()
            && &path == key
        {
            self.node(key)?.sddl = undone;
        }
        Ok(())
    }

    fn has_link_value(&mut self, key: &String) -> Result<bool, Miss> {
        Ok(self.node(key)?.link_value)
    }

    fn delete_link_value(&mut self, key: &String) -> Result<(), Miss> {
        self.node(key)?.link_value = false;
        Ok(())
    }

    fn sddl(&mut self, key: &String) -> Result<String, Miss> {
        Ok(self.node(key)?.sddl.clone())
    }

    fn set_sddl(&mut self, key: &String, sddl: &str) -> Result<(), Miss> {
        self.node(key)?.sddl = sddl.to_owned();
        self.set.push((key.clone(), sddl.to_owned()));
        Ok(())
    }

    fn subkeys(&mut self, key: &String) -> Result<Vec<String>, Miss> {
        self.node(key)?;
        let prefix = format!("{key}\\");
        Ok(self
            .keys
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('\\'))
            .map(str::to_owned)
            .collect())
    }

    fn delete(&mut self, key: String) -> Result<(), Miss> {
        if self.fail_delete.as_deref() == Some(key.as_str()) {
            return Err(Miss::Failed("NtDeleteKey failed (0xc0000022)".to_owned()));
        }
        if !self.subkeys(&key)?.is_empty() {
            return Err(Miss::Failed(
                "NtDeleteKey failed: the key has subkeys".to_owned(),
            ));
        }
        self.keys.remove(&key);
        self.deleted.push(key);
        Ok(())
    }

    fn reset_install_dir(&mut self, key: &String) -> Result<Option<String>, Miss> {
        let location = self.install_location.clone();
        let node = self.node(key)?;
        node.values.retain(|value| value != "(default)");
        if location.is_some() {
            node.values.push("(default)".to_owned());
        }
        self.install_resets.push(key.clone());
        Ok(location)
    }

    fn local_domain(&mut self) -> Result<[u32; 3], String> {
        Ok([1, 2, 3])
    }

    fn in_administrators(&mut self, sid: &str) -> Result<bool, String> {
        self.admins.get(sid).cloned().unwrap_or(Ok(false))
    }

    fn spell_out(&mut self, alias: &str) -> Result<String, String> {
        match alias {
            "LA" => Ok("S-1-5-21-1-2-3-500".to_owned()),
            "LG" => Ok("S-1-5-21-1-2-3-501".to_owned()),
            _ => Err("no such alias".to_owned()),
        }
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
fn the_pinned_sddls_are_the_ones_the_module_names() {
    assert_eq!(
        VENDOR_SDDL,
        "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)"
    );
    assert_eq!(
        INHERITED_SDDL,
        "O:SYD:AI(A;OICIID;KA;;;SY)(A;OICIID;KA;;;BA)(A;OICIID;KR;;;BU)"
    );
    assert_eq!(
        record_pinned(),
        "O:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)"
    );
}

#[test]
fn a_missing_root_is_made_and_taken_over() {
    let mut tree = FakeTree::with(None, &[]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report, Report::default());
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert_eq!(tree.set, [(VENDOR_KEY.to_owned(), VENDOR_SDDL.to_owned())]);
}

#[test]
fn a_weak_root_is_taken_over_and_its_user_owned_subkeys_go_bottom_up() {
    let mut tree = FakeTree::with(
        Some(WEAK),
        &[
            ("Planted", USER_KEY, false),
            ("Planted\\Nested", ADMIN_KEY, false),
        ],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.weaker_from.as_deref(), Some(WEAK));
    assert_eq!(
        tree.set.first(),
        Some(&(VENDOR_KEY.to_owned(), VENDOR_SDDL.to_owned())),
        "the root is taken over before anything under it is read"
    );
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert_eq!(report.planted, [under("Planted")]);
    assert_eq!(tree.deleted, [under("Planted\\Nested"), under("Planted")]);
    assert!(
        tree.set
            .contains(&(under("Planted"), LOCKED_SDDL.to_owned()))
    );
}

#[test]
fn a_real_link_root_is_refused_and_nothing_under_it_is_touched() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", USER_KEY, false)]);
    tree.keys.get_mut(VENDOR_KEY).unwrap().link = true;
    tree.keys.get_mut(VENDOR_KEY).unwrap().link_value = true;
    assert_eq!(ensure(&mut tree, VENDOR_KEY), Err(VendorFault::Link));
    assert_eq!(tree.set, [(VENDOR_KEY.to_owned(), VENDOR_SDDL.to_owned())]);
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(tree.sddl_of(&under("Planted")), USER_KEY);
}

#[test]
fn a_fake_link_value_on_the_root_is_deleted_and_set_up_goes_on() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Kept", ADMIN_KEY, false)]);
    tree.keys.get_mut(VENDOR_KEY).unwrap().link_value = true;
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert!(report.fake_root_link);
    assert!(!tree.keys[VENDOR_KEY].link_value);
    assert_eq!(report.taken_over, [under("Kept")]);
}

#[test]
fn a_failed_root_take_over_refuses() {
    let mut tree = FakeTree::with(Some(WEAK), &[("Planted", USER_KEY, false)]);
    tree.fail_take = Some(VENDOR_KEY.to_owned());
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::Secure(_))), "{result:?}");
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(tree.sddl_of(&under("Planted")), USER_KEY);
}

#[test]
fn a_failed_subkey_take_over_refuses() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Planted", USER_KEY, false)]);
    tree.fail_take = Some(under("Planted"));
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::Secure(_))), "{result:?}");
}

#[test]
fn a_link_child_and_a_key_with_a_fake_link_value_are_deleted() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("Link", ADMIN_KEY, true), ("Fake", ADMIN_KEY, false)],
    );
    tree.keys.get_mut(&under("Fake")).unwrap().link_value = true;
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.removed, [under("Fake"), under("Link")]);
    assert!(!tree.has(&under("Link")) && !tree.has(&under("Fake")));
}

#[test]
fn a_failed_link_delete_refuses() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Link", ADMIN_KEY, true)]);
    tree.fail_delete = Some(under("Link"));
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(matches!(result, Err(VendorFault::CleanUp(_))), "{result:?}");
}

#[test]
fn an_unresolved_owner_keeps_its_key_but_not_sandbox() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("Kept", GHOST_KEY, false), ("Sandbox", GHOST_KEY, false)],
    );
    tree.values(&under("Kept"), &["Value"]);
    tree.values(&under("Sandbox"), &["PoolSids"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.sddl_of(&under("Kept")), INHERITED_SDDL);
    assert_eq!(tree.keys[&under("Kept")].values, ["Value"]);
    assert_eq!(
        report.unresolved,
        [(under("Kept"), "the account has no name".to_owned())]
    );
    assert_eq!(report.planted, [under("Sandbox")]);
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
    assert_eq!(tree.keys[&under("Sandbox")].values, Vec::<String>::new());
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
    tree.values(&under("Sandbox"), &["PoolSids"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
    assert_eq!(tree.keys[&under("Sandbox")].values, Vec::<String>::new());
    assert!(!tree.has(&under("Sandbox\\Inner")));
    assert_eq!(tree.deleted, [under("Sandbox\\Inner"), under("Sandbox")]);
    assert_eq!(report.resecured, Vec::<String>::new());
}

#[test]
fn a_sandbox_with_a_link_value_is_deleted_and_recreated() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", ADMIN_KEY, false)]);
    tree.keys.get_mut(&under("Sandbox")).unwrap().link_value = true;
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.removed, [under("Sandbox")]);
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert!(!tree.keys[&under("Sandbox")].link_value);
}

#[test]
fn an_admin_owned_sandbox_only_admins_could_write_keeps_its_values_and_gets_its_sd() {
    let drifted = "O:BAD:P(A;CI;KA;;;BA)(A;CI;KR;;;BU)";
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", drifted, false)]);
    tree.values(&under("Sandbox"), &["PoolSids"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.resecured, [under("Sandbox")]);
    assert!(report.recreated.is_empty() && tree.deleted.is_empty());
    assert_eq!(tree.keys[&under("Sandbox")].values, ["PoolSids"]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
}

#[test]
fn a_kept_key_others_could_write_to_is_logged() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Kept", "O:SYD:P(A;;KW;;;BU)", false)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.sddl_of(&under("Kept")), INHERITED_SDDL);
    assert_eq!(report.others_could_write, [under("Kept")]);
    assert_eq!(
        tree.install_resets,
        Vec::<String>::new(),
        "only the installer's key"
    );
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
    assert_eq!(report.others_could_write, Vec::<String>::new());
}

#[test]
fn plants_added_mid_walk_are_deleted() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("A", ADMIN_KEY, false), ("B", ADMIN_KEY, false)],
    );
    tree.events = vec![
        (
            under("A"),
            Event::Plant(under("B\\Plant"), USER_KEY.to_owned(), false),
        ),
        (
            under("A"),
            Event::Plant(under("B\\LinkPlant"), ADMIN_KEY.to_owned(), true),
        ),
    ];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.planted, [under("B\\Plant")]);
    assert_eq!(report.removed, [under("B\\LinkPlant")]);
    assert!(!tree.has(&under("B\\Plant")) && !tree.has(&under("B\\LinkPlant")));
}

#[test]
fn keys_deleted_under_the_walk_are_done_and_sandbox_comes_back() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("Gone", ADMIN_KEY, false),
            ("Gone\\Child", ADMIN_KEY, false),
            ("Sandbox", ADMIN_KEY, false),
        ],
    );
    tree.events = vec![
        (under("Gone"), Event::Vanish(under("Gone"))),
        (under("Sandbox"), Event::Vanish(under("Sandbox"))),
    ];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert!(!tree.has(&under("Gone")));
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
}

#[test]
fn a_link_value_written_mid_walk_through_an_open_handle_deletes_the_key() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", ADMIN_KEY, false)]);
    tree.events = vec![(under("Sandbox"), Event::LinkValue(under("Sandbox")))];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.removed, [under("Sandbox")]);
    assert_eq!(report.recreated, [under("Sandbox")]);
}

#[test]
fn a_root_deleted_under_the_walk_is_walked_again() {
    let mut tree = FakeTree::with(Some(WEAK), &[("A", ADMIN_KEY, false)]);
    tree.events = vec![(under("A"), Event::Vanish(VENDOR_KEY.to_owned()))];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.created, [VENDOR_KEY.to_owned()]);
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
    assert_eq!(report, Report::default());
}

#[test]
fn a_key_forty_levels_deep_is_taken_over_not_refused() {
    let mut path = String::from("K");
    let mut children = Vec::new();
    for _ in 0..40 {
        children.push((path.clone(), ADMIN_KEY.to_owned(), false));
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
    assert_eq!(tree.sddl_of(&under(&deepest)), INHERITED_SDDL);
}
