//! The take-over's steps against a model of Windows' access check, and the structural checks.

use super::super::security::{
    KEY_ALL, READ_CONTROL, Security, WRITE_DAC, WRITE_OWNER, take_over_steps,
};
use super::super::{INHERITED_SDDL, LOCKED_SDDL, VENDOR_SDDL};

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
    assert!(pinned.is_vendor_set());
    assert!(Security::parse(INHERITED_SDDL).is_vendor_set());
    assert!(
        !Security::parse("O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KA;;;BU)").is_vendor_set()
    );
    assert!(
        !Security::parse("O:SYD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;BU)(A;;KR;;;WD)").is_vendor_set()
    );
    assert!(
        !Security::parse("O:SYD:P(D;;KA;;;BU)(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;BU)").is_vendor_set()
    );
}

/// The access an elevated administrator (Administrators in its token, `SeTakeOwnershipPrivilege`
/// enabled, not SYSTEM) gets on a key with `sd`, asking `want`, as Windows checks it: the owner
/// has `READ_CONTROL` and `WRITE_DAC` first, the privilege `WRITE_OWNER`, then the DACL's ACEs in
/// order, a DENY one refusing what isn't granted yet. `KA` and `KR` are the rights the keys use.
fn admin_gets(sd: &Security, want: u32) -> bool {
    let token = ["S-1-5-32-544", "S-1-5-11", "S-1-1-0", "S-1-5-32-545"];
    let mut granted = WRITE_OWNER;
    if token.contains(&sd.owner.as_str()) {
        granted |= READ_CONTROL | WRITE_DAC;
    }
    let mut denied = 0;
    for ace in &sd.aces {
        if !token.contains(&ace.sid.as_str()) {
            continue;
        }
        let rights = match ace.rights.as_str() {
            "KA" => KEY_ALL,
            "KR" => 0x0002_0019,
            other => panic!("rights {other} aren't modelled"),
        };
        match ace.kind.as_str() {
            "D" => denied |= rights & !granted,
            _ => granted |= rights & !denied,
        }
    }
    granted & want == want
}

/// Applies `sddl`'s parts to `sd`, as `RegSetKeySecurity` writes them.
fn apply(sd: &mut Security, sddl: &str) {
    let parts = Security::parse(sddl);
    if sddl.contains("O:") {
        sd.owner = parts.owner;
    }
    if sddl.contains("D:") {
        sd.protected = parts.protected;
        sd.aces = parts.aces;
    }
}

#[test]
fn a_key_that_denies_system_and_administrators_is_taken_over() {
    let mut sd = Security::parse("O:S-1-5-21-1-2-3-1001D:P(D;;KA;;;SY)(D;;KA;;;BA)");
    assert!(!admin_gets(&sd, KEY_ALL), "a full open is refused");
    assert!(
        !admin_gets(&sd, READ_CONTROL | WRITE_OWNER),
        "and so is the old READ_CONTROL with WRITE_OWNER"
    );
    for step in take_over_steps(LOCKED_SDDL) {
        assert!(
            admin_gets(&sd, step.access),
            "{step:?} is refused on {sd:?}"
        );
        apply(&mut sd, &step.sddl);
    }
    assert_eq!(sd, Security::parse(LOCKED_SDDL));
    let steps = take_over_steps(LOCKED_SDDL);
    assert_eq!(
        steps[0].access, WRITE_OWNER,
        "the first open asks WRITE_OWNER alone"
    );
    assert_eq!(steps[1].access, READ_CONTROL | WRITE_DAC);
}
