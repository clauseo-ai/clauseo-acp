//! The installer's key, `Clauseo\Clauseo`, whose install folder the uninstaller deletes: kept
//! only when SYSTEM, Administrators or an account in Administrators owned it and no one else
//! could write it; otherwise deleted and made again with the uninstall key's folder.

use super::owners::TRUSTED_INSTALLER;
use super::{ADMIN_KEY, ADMIN_USER_KEY, FakeTree, GHOST_KEY, USER_KEY, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::{INHERITED_SDDL, VENDOR_SDDL, ensure};

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

/// Asserts the installer's key was deleted and made again with the uninstall key's folder.
fn assert_remade(tree: &FakeTree, report: &crate::windows::setup::vendor_key::Report) {
    assert_eq!(tree.deleted, [under("Clauseo")]);
    assert_eq!(
        report.install_dir_reset,
        Some((under("Clauseo"), Some(FOLDER.to_owned())))
    );
    assert_eq!(tree.keys[&under("Clauseo")].values, ["(default)"]);
    assert_eq!(tree.sddl_of(&under("Clauseo")), INHERITED_SDDL);
    assert!(report.recreated.is_empty(), "the pinned keys' list");
}

#[test]
fn an_installer_key_admins_own_and_only_admins_could_write_is_kept_with_its_values() {
    for sddl in [ADMIN_KEY, ADMIN_USER_KEY] {
        let mut tree = installer_tree(sddl);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(tree.deleted, Vec::<String>::new(), "{sddl}");
        assert_eq!(report.install_dir_reset, None);
        assert_eq!(tree.install_resets, Vec::<String>::new());
        assert_eq!(
            tree.keys[&under("Clauseo")].values,
            ["(default)", "Installer Language"]
        );
        assert_eq!(report.taken_over, [under("Clauseo")]);
    }
}

#[test]
fn an_installer_key_others_could_write_is_made_again_so_no_held_handle_outlives_it() {
    let mut tree = installer_tree(ADMIN_USERS_WRITE);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.untrusted_installer, [under("Clauseo")]);
    assert_remade(&tree, &report);
}

#[test]
fn a_domain_owned_installer_key_only_admins_could_write_is_made_again() {
    let mut tree = installer_tree(DOMAIN_KEY);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.untrusted_installer, [under("Clauseo")]);
    assert_eq!(report.unresolved, Vec::<(String, String)>::new());
    assert_remade(&tree, &report);
}

#[test]
fn installer_keys_unresolved_or_os_owned_are_made_again_while_other_such_keys_are_kept() {
    let ti = format!("O:{TRUSTED_INSTALLER}D:P(A;;KA;;;BA)");
    for sddl in [GHOST_KEY, ti.as_str(), "O:S-1-5-32-551D:P(A;;KA;;;BA)"] {
        let mut tree = installer_tree(sddl);
        tree.insert(under("Other"), sddl, false);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.untrusted_installer, [under("Clauseo")], "{sddl}");
        assert_remade(&tree, &report);
        assert_eq!(report.taken_over, [under("Other")], "{sddl}");
        assert_eq!(tree.sddl_of(&under("Other")), INHERITED_SDDL);
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
