//! Who owned a key: SYSTEM, Administrators and the OS's service accounts are kept, a well-known
//! SID ordinary users hold is a planter, an alias is spelled out first, a domain account or
//! another well-known SID is unresolved (kept, unless the SYSTEM side trusts the key), and a
//! pinned key others could write is made again even when an administrator owns it.

use super::{FakeTree, record_pinned, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::security::Security;
use crate::windows::setup::vendor_key::{
    INHERITED_SDDL, VENDOR_SDDL, VendorFault, ensure, walk_root,
};

/// A key Administrators own whose DACL let Users write.
const ADMIN_USERS_WRITE: &str = "O:BAD:P(A;;KA;;;BA)(A;;KA;;;BU)";

/// `TrustedInstaller`'s SID.
pub(super) const TRUSTED_INSTALLER: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

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
fn os_owners_are_kept_user_held_owners_planted_and_other_well_known_ones_unresolved() {
    let ti = format!("O:{TRUSTED_INSTALLER}D:P(A;;KA;;;BA)");
    let service = "O:S-1-5-80-1-2-3-4-5D:P(A;;KA;;;BA)";
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[
            ("TiOwned", &ti, false),
            ("Network", "O:NSD:P(A;;KA;;;BA)", false),
            ("Service", service, false),
            ("Users", "O:BUD:P(A;;KA;;;BA)", false),
            ("Everyone", "O:S-1-1-0D:P(A;;KA;;;BA)", false),
            ("Signed", "O:AUD:P(A;;KA;;;BA)", false),
            ("Backup", "O:S-1-5-32-551D:P(A;;KA;;;BA)", false),
            ("Admin", "O:LAD:P(A;;KA;;;BA)", false),
            ("Guest", "O:LGD:P(A;;KA;;;BA)", false),
            ("Odd", "O:XXD:P(A;;KA;;;BA)", false),
        ],
    );
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(
        report.planted,
        [
            under("Everyone"),
            under("Guest"),
            under("Signed"),
            under("Users")
        ]
    );
    assert_eq!(
        report.taken_over,
        [
            under("Admin"),
            under("Backup"),
            under("Network"),
            under("Odd"),
            under("Service"),
            under("TiOwned")
        ]
    );
    assert_eq!(report.admin_owned, [under("Admin")]);
    assert_eq!(
        report.unresolved,
        [
            (
                under("Backup"),
                "a well-known SID set-up doesn't judge".to_owned()
            ),
            (under("Odd"), "the alias XX: no such alias".to_owned())
        ]
    );
    for kept in ["TiOwned", "Network", "Service", "Backup", "Admin", "Odd"] {
        assert_eq!(tree.sddl_of(&under(kept)), INHERITED_SDDL, "{kept}");
    }
}

#[test]
fn a_sandbox_a_service_owns_is_planted_and_made_again_empty() {
    let ti = format!("O:{TRUSTED_INSTALLER}D:P(A;;KA;;;BA)");
    for sddl in [
        ti.as_str(),
        "O:NSD:P(A;;KA;;;BA)",
        "O:S-1-5-80-1-2-3D:P(A;;KA;;;BA)",
    ] {
        let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", sddl, false)]);
        tree.values(&under("Sandbox"), &["PoolSids"]);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.planted, [under("Sandbox")], "{sddl}");
        assert_eq!(report.recreated, [under("Sandbox")], "{sddl}");
        assert_eq!(tree.keys[&under("Sandbox")].values, Vec::<String>::new());
    }
}

#[test]
fn a_null_dacl_and_a_conditional_allow_let_others_write() {
    let null = "O:BAD:NO_ACCESS_CONTROL";
    let conditional = "O:BAD:P(A;;KA;;;BA)(XA;;KA;;;BU;(WIN://SYSAPPID Contains \"x\"))";
    let object = "O:BAD:P(A;;KA;;;BA)(OA;;KA;;;BU)";
    for sddl in [null, "O:BA", conditional, object] {
        assert!(Security::parse(sddl).others_may_write(), "{sddl}");
        let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("Sandbox", sddl, false)]);
        tree.values(&under("Sandbox"), &["PoolSids"]);
        let report = ensure(&mut tree, VENDOR_KEY).unwrap();
        assert_eq!(report.writable_pinned, [under("Sandbox")], "{sddl}");
        assert_eq!(tree.keys[&under("Sandbox")].values, Vec::<String>::new());
    }
    assert!(!Security::parse("O:BAD:P(A;;KA;;;BA)(XA;;KR;;;BU;(x))").others_may_write());
}

#[test]
fn set_up_walks_only_the_vendor_key() {
    assert_eq!(walk_root(VENDOR_KEY, false), Ok(()));
    assert_eq!(walk_root("software\\clauseo", false), Ok(()));
    for root in ["SOFTWARE", "SOFTWARE\\ClauseoTestAcct", ""] {
        assert!(
            matches!(walk_root(root, false), Err(VendorFault::Open(_))),
            "{root}"
        );
    }
    assert_eq!(walk_root("SOFTWARE\\ClauseoTestAcct", true), Ok(()));
}
