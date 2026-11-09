//! Who owned a key: a well-known group is a planter, a domain account is unresolved (kept,
//! unless the SYSTEM side trusts the key), a pinned key others could write is made again even
//! when an administrator owns it, and the installer's key others could write gets its install
//! folder from the uninstall key.

use super::{ADMIN_KEY, FakeTree, record_pinned, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::{INHERITED_SDDL, VENDOR_SDDL, ensure};

/// A key a domain account owns, with only Administrators let in.
const DOMAIN_KEY: &str = "O:S-1-5-21-9-9-9-1105D:P(A;;KA;;;BA)";

/// A key Administrators own whose DACL let Users write.
const ADMIN_USERS_WRITE: &str = "O:BAD:P(A;;KA;;;BA)(A;;KA;;;BU)";

#[test]
fn a_domain_owned_installer_key_is_kept_and_a_domain_owned_sandbox_is_not() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("Clauseo", DOMAIN_KEY, false),
            ("Sandbox", DOMAIN_KEY, false),
        ],
    );
    // Membership would say no, as an unreachable domain controller can.
    tree.admins
        .insert("S-1-5-21-9-9-9-1105".to_owned(), Ok(false));
    tree.values(&under("Clauseo"), &["(default)"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(tree.sddl_of(&under("Clauseo")), INHERITED_SDDL);
    assert_eq!(tree.keys[&under("Clauseo")].values, ["(default)"]);
    assert_eq!(
        report.unresolved,
        [(under("Clauseo"), "not a local account".to_owned())]
    );
    assert_eq!(report.planted, [under("Sandbox")]);
    assert_eq!(report.recreated, [under("Sandbox")]);
}

#[test]
fn an_admin_owned_sandbox_users_could_write_is_made_again() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", ADMIN_USERS_WRITE, false)]);
    tree.values(&under("Sandbox"), &["PoolSids"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.writable_pinned, [under("Sandbox")]);
    assert_eq!(report.recreated, [under("Sandbox")]);
    assert_eq!(tree.deleted, [under("Sandbox")]);
    assert_eq!(tree.keys[&under("Sandbox")].values, Vec::<String>::new());
    assert_eq!(tree.sddl_of(&under("Sandbox")), record_pinned());
}

#[test]
fn an_installer_key_others_could_write_gets_its_folder_from_the_uninstall_key() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Clauseo", ADMIN_USERS_WRITE, false)]);
    tree.install_location = Some("C:\\Program Files\\Clauseo".to_owned());
    tree.values(&under("Clauseo"), &["(default)", "Installer Language"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(
        report.install_dir_reset,
        Some((
            under("Clauseo"),
            Some("C:\\Program Files\\Clauseo".to_owned())
        ))
    );
    assert_eq!(report.others_could_write, [under("Clauseo")]);
    assert_eq!(
        tree.deleted,
        Vec::<String>::new(),
        "the installer's key is kept"
    );
}

#[test]
fn an_installer_key_others_could_write_loses_its_folder_without_an_uninstall_key() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Clauseo", ADMIN_USERS_WRITE, false)]);
    tree.values(&under("Clauseo"), &["(default)", "Installer Language"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.install_dir_reset, Some((under("Clauseo"), None)));
    assert_eq!(tree.keys[&under("Clauseo")].values, ["Installer Language"]);
}

#[test]
fn an_installer_key_only_admins_could_write_keeps_its_folder() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Clauseo", ADMIN_KEY, false)]);
    tree.values(&under("Clauseo"), &["(default)"]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.install_dir_reset, None);
    assert_eq!(tree.install_resets, Vec::<String>::new());
    assert_eq!(tree.keys[&under("Clauseo")].values, ["(default)"]);
}

#[test]
fn a_well_known_owner_is_a_planter_and_an_alias_set_up_cant_spell_out_is_unresolved() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("Users", "O:BUD:P(A;;KA;;;BA)", false),
            ("Everyone", "O:S-1-1-0D:P(A;;KA;;;BA)", false),
            ("Network", "O:NSD:P(A;;KA;;;BA)", false),
            ("Admin", "O:LAD:P(A;;KA;;;BA)", false),
        ],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(
        report.planted,
        [under("Everyone"), under("Network"), under("Users")]
    );
    assert_eq!(
        report.unresolved,
        [(under("Admin"), "an alias set-up can't spell out".to_owned())]
    );
    assert_eq!(tree.sddl_of(&under("Admin")), INHERITED_SDDL);
}
