//! The installer's key, `Clauseo\Clauseo`, whose install folder the uninstaller deletes: deleted on
//! every walk, whatever its owner and DACL, so no handle opened while it was weak outlives it,
//! and made again with the uninstall key's folder.

use super::owners::TRUSTED_INSTALLER;
use super::{ADMIN_KEY, ADMIN_USER_KEY, FakeTree, GHOST_KEY, USER_KEY, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::{INHERITED_SDDL, Report, VENDOR_SDDL, ensure};

/// A key a domain account owns, with only Administrators let in.
const DOMAIN_KEY: &str = "O:S-1-5-21-9-9-9-1105D:P(A;;KA;;;BA)";

/// A key Administrators own whose DACL let Users write.
const ADMIN_USERS_WRITE: &str = "O:BAD:P(A;;KA;;;BA)(A;;KA;;;BU)";

/// The uninstall key's folder in these tests.
const FOLDER: &str = "C:\\Program Files\\Clauseo";

/// The tree with only the installer's key, `sddl`, holding a planted folder and another value,
/// and the uninstall key's folder.
fn installer_tree(sddl: &str) -> FakeTree {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Clauseo", sddl, false)]);
    tree.install_location = Some(FOLDER.to_owned());
    tree.values(&under("Clauseo"), &["(default)", "Installer Language"]);
    tree
}

/// Asserts the installer's key was deleted and made again with the uninstall key's folder only.
fn assert_remade(tree: &FakeTree, report: &Report, sddl: &str) {
    assert_eq!(tree.deleted, [under("Clauseo")], "{sddl}");
    assert_eq!(
        report.install_dir_reset,
        Some((under("Clauseo"), Some(FOLDER.to_owned()))),
        "{sddl}"
    );
    assert_eq!(tree.keys[&under("Clauseo")].values, ["(default)"], "{sddl}");
    assert_eq!(tree.sddl_of(&under("Clauseo")), INHERITED_SDDL, "{sddl}");
    assert!(report.recreated.is_empty(), "the pinned keys' list");
    assert!(report.taken_over.is_empty(), "{sddl}");
}

#[test]
fn every_installer_key_is_made_again_whatever_its_owner_and_dacl() {
    let ti = format!("O:{TRUSTED_INSTALLER}D:P(A;;KA;;;BA)");
    let null = "O:BAD:NO_ACCESS_CONTROL";
    let conditional = "O:BAD:P(A;;KA;;;BA)(XA;;KA;;;BU;(WIN://SYSAPPID Contains \"x\"))";
    for sddl in [
        ADMIN_KEY,
        ADMIN_USER_KEY,
        DOMAIN_KEY,
        ADMIN_USERS_WRITE,
        GHOST_KEY,
        ti.as_str(),
        "O:S-1-5-32-551D:P(A;;KA;;;BA)",
        null,
        conditional,
    ] {
        let mut tree = installer_tree(sddl);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_remade(&tree, &report, sddl);
    }
}

#[test]
fn a_planted_installer_key_is_made_again_without_a_folder_when_the_uninstall_key_has_none() {
    let mut tree = installer_tree(USER_KEY);
    tree.install_location = None;
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.planted, [under("Clauseo")]);
    assert_eq!(report.install_dir_reset, Some((under("Clauseo"), None)));
    assert_eq!(tree.keys[&under("Clauseo")].values, Vec::<String>::new());
}

#[test]
fn other_keys_with_the_same_owners_are_kept() {
    let ti = format!("O:{TRUSTED_INSTALLER}D:P(A;;KA;;;BA)");
    for sddl in [ADMIN_KEY, GHOST_KEY, ti.as_str()] {
        let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Other", sddl, false)]);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.taken_over, [under("Other")], "{sddl}");
        assert_eq!(tree.deleted, Vec::<String>::new(), "{sddl}");
    }
}

#[test]
fn only_the_top_level_installer_key_is_held_to_its_rule() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("Other", ADMIN_KEY, false),
            ("Other\\Clauseo", GHOST_KEY, false),
        ],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.deleted, Vec::<String>::new());
    assert_eq!(report.install_dir_reset, None);
}
