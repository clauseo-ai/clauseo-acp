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

/// The access an elevated administrator (Administrators in its token, not SYSTEM) gets on a
/// key with `sd`, asking `want`, as Windows checks it. With `restore` intent and the backup
/// and restore privileges, the DACL isn't asked at all. Otherwise the owner has `READ_CONTROL`
/// and `WRITE_DAC` first unless an OWNER RIGHTS ACE replaces them, then the DACL's ACEs in
/// order, a DENY one refusing what isn't granted yet. `KA` and `KR` are the rights modelled.
fn admin_gets(sd: &Security, want: u32, restore: bool) -> bool {
    if restore {
        return want & !(READ_CONTROL | WRITE_DAC | WRITE_OWNER) == 0;
    }
    let token = ["S-1-5-32-544", "S-1-5-11", "S-1-1-0", "S-1-5-32-545"];
    let is_owner = token.contains(&sd.owner.as_str());
    let owner_rights = sd.aces.iter().any(|ace| ace.sid == "OW");
    let mut granted = 0;
    if is_owner && !owner_rights {
        granted |= READ_CONTROL | WRITE_DAC;
    }
    let mut denied = 0;
    for ace in &sd.aces {
        let applies = token.contains(&ace.sid.as_str()) || (ace.sid == "OW" && is_owner);
        if !applies {
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

/// Applies `sddl` to `sd`, as `RegSetKeySecurity` writes it.
fn apply(sd: &mut Security, sddl: &str) {
    *sd = Security::parse(sddl);
}

/// Runs the take-over's steps on `sd`: whether each open is granted, and the SD it ends with.
fn take_over(mut sd: Security) -> Result<Security, String> {
    for step in take_over_steps(LOCKED_SDDL) {
        if !admin_gets(&sd, step.access, step.restore) {
            return Err(format!("{step:?} is refused on {sd:?}"));
        }
        if let Some(sddl) = &step.sddl {
            apply(&mut sd, sddl);
        }
    }
    Ok(sd)
}

#[test]
fn a_key_that_denies_system_and_administrators_is_taken_over() {
    let sd = Security::parse("O:S-1-5-21-1-2-3-1001D:P(D;;KA;;;SY)(D;;KA;;;BA)");
    assert!(!admin_gets(&sd, KEY_ALL, false), "a full open is refused");
    assert_eq!(take_over(sd), Ok(Security::parse(LOCKED_SDDL)));
}

#[test]
fn a_key_whose_owner_rights_are_cut_down_is_taken_over() {
    let sd = Security::parse("O:BAD:P(A;;KR;;;OW)");
    assert!(
        !admin_gets(&sd, READ_CONTROL | WRITE_DAC, false),
        "an owner's WRITE_DAC is gone, so ownership alone wouldn't do"
    );
    assert_eq!(take_over(sd), Ok(Security::parse(LOCKED_SDDL)));
}

#[test]
fn the_owner_and_dacl_are_written_in_one_step_with_restore_intent() {
    let steps = take_over_steps(VENDOR_SDDL);
    assert!(steps[0].restore);
    assert_eq!(steps[0].access, READ_CONTROL | WRITE_OWNER | WRITE_DAC);
    assert_eq!(steps[0].sddl.as_deref(), Some(VENDOR_SDDL));
    assert!(!steps[1].restore && steps[1].sddl.is_none());
}
