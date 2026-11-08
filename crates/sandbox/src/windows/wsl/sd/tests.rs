//! The launch permission's edits, on the stock MSI value (`O:BAG:BAD:(A;;CCDCSW;;;AU)
//! (A;;CCDCSW;;;PS)(A;;CCDCSW;;;SY)`, 120 bytes, the VM's R5c read) laid out both ways Windows
//! writes a self-relative descriptor, and on what must be refused.

use super::*;

/// `BUILTIN\Administrators`.
const BA: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];

/// The pool group in the tests.
fn group() -> Sid {
    Sid::parse("S-1-5-21-7-8-9-1009").unwrap()
}

/// `S-1-5-<rid>`'s bytes.
fn nt(rid: u8) -> Vec<u8> {
    vec![1, 1, 0, 0, 0, 0, 0, 5, rid, 0, 0, 0]
}

/// An entry by hand.
fn entry(kind: u8, flags: u8, mask: u32, sid: &[u8]) -> Vec<u8> {
    let mut out = vec![kind, flags, u8::try_from(8 + sid.len()).unwrap(), 0];
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(sid);
    out
}

/// An ACL by hand.
fn acl(entries: &[Vec<u8>]) -> Vec<u8> {
    let size = 8 + entries.iter().map(Vec::len).sum::<usize>();
    let mut out = vec![2, 0, u8::try_from(size).unwrap(), 0];
    out.extend_from_slice(&[u8::try_from(entries.len()).unwrap(), 0, 0, 0]);
    for each in entries {
        out.extend_from_slice(each);
    }
    out
}

/// The stock DACL: `CCDCSW` (0xB) for AU, PS and SY.
fn stock_dacl() -> Vec<u8> {
    acl(&[
        entry(0, 0, 0xB, &nt(11)),
        entry(0, 0, 0xB, &nt(10)),
        entry(0, 0, 0xB, &nt(18)),
    ])
}

/// A descriptor by hand: `control`, then the parts in the order given, each with its offset.
fn laid_out(control: u16, parts: &[(usize, &[u8])]) -> Vec<u8> {
    let mut out = vec![1, 0];
    out.extend_from_slice(&control.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for (offset_at, bytes) in parts {
        let offset = u32::try_from(out.len()).unwrap().to_le_bytes();
        out[*offset_at..*offset_at + 4].copy_from_slice(&offset);
        out.extend_from_slice(bytes);
    }
    out
}

/// The stock value as `ConvertStringSecurityDescriptorToSecurityDescriptorW` lays it out:
/// owner, group, DACL.
fn stock_owner_first() -> Vec<u8> {
    laid_out(0x8004, &[(4, &BA), (8, &BA), (16, &stock_dacl())])
}

/// The stock value as `MakeSelfRelativeSD` lays it out: DACL, owner, group.
fn stock_dacl_first() -> Vec<u8> {
    laid_out(0x8004, &[(16, &stock_dacl()), (4, &BA), (8, &BA)])
}

#[test]
fn the_stock_value_takes_the_deny_first_and_keeps_the_rest() {
    for stock in [stock_owner_first(), stock_dacl_first()] {
        assert_eq!(stock.len(), 120);
        let sd = LaunchSd::parse(&stock).unwrap();
        assert_eq!(sd.to_bytes(), stock, "read and written back unchanged");
        assert!(!sd.denies_first(&group()));
        let merged = sd.with_deny(&group()).unwrap();
        let bytes = merged.to_bytes();
        // O:BAG:BAD:(D;;CCDCLCSWRP;;;G)(A;;CCDCSW;;;AU)(A;;CCDCSW;;;PS)(A;;CCDCSW;;;SY)
        let deny = entry(1, 0, 0x1F, &sid_bytes(&group()));
        let mut aces = vec![deny];
        aces.extend(sd.aces.clone());
        assert_eq!(merged.aces, aces);
        assert_eq!(bytes.len(), 120 + 36);
        let again = LaunchSd::parse(&bytes).unwrap();
        assert_eq!(again, merged);
        assert_eq!(
            (again.owner.as_slice(), again.group.as_slice()),
            (&BA[..], &BA[..])
        );
        assert_eq!(again.order, sd.order, "the parts keep their places");
        assert!(again.denies_first(&group()));
        assert_eq!(bytes[2..4], stock[2..4], "the control word");
    }
}

#[test]
fn a_deny_already_first_is_left_and_a_weaker_one_is_not_enough() {
    let sd = LaunchSd::parse(&stock_owner_first()).unwrap();
    let merged = sd.with_deny(&group()).unwrap();
    assert_eq!(merged.with_deny(&group()), None, "idempotent");
    // All of 0x1F is enough.
    let mut enough = sd.clone();
    enough
        .aces
        .insert(0, entry(1, 0, 0x1F, &sid_bytes(&group())));
    assert!(enough.denies_first(&group()));
    assert_eq!(enough.with_deny(&group()), None);
    // Execute alone, the local rights alone (0xB, without the remote ones), an inherit-only
    // deny, or another group's deny is not.
    let other = Sid::parse("S-1-5-21-7-8-9-1010").unwrap();
    for weak in [
        entry(1, 0, 0x1, &sid_bytes(&group())),
        entry(1, 0, 0xB, &sid_bytes(&group())),
        entry(1, INHERIT_ONLY, 0x1F, &sid_bytes(&group())),
        entry(1, 0, 0x1F, &sid_bytes(&other)),
    ] {
        let mut sd = sd.clone();
        sd.aces.insert(0, weak.clone());
        assert!(!sd.denies_first(&group()), "{weak:?}");
        let merged = sd.with_deny(&group()).unwrap();
        assert_eq!(merged.aces[1], weak, "kept, after ours");
        assert!(merged.denies_first(&group()));
    }
}

#[test]
fn a_deny_after_an_allow_fails_the_check_and_is_put_first() {
    let sd = LaunchSd::parse(&stock_owner_first()).unwrap();
    let mut late = sd.clone();
    late.aces.push(entry(1, 0, 0x1F, &sid_bytes(&group())));
    let bytes = late.to_bytes();
    let late = LaunchSd::parse(&bytes).unwrap();
    assert!(late.names_deny(&group()));
    assert!(!late.denies_first(&group()), "an allow comes first");
    let merged = late.with_deny(&group()).unwrap();
    assert!(merged.denies_first(&group()));
    assert_eq!(merged.aces.len(), 5, "every entry kept");
    // Removing takes both of ours off.
    let (rest, dropped) = merged.without_denies(&group());
    assert_eq!((rest, dropped), (sd, 2));
}

#[test]
fn removing_the_deny_gives_back_the_stock_bytes_exactly() {
    for stock in [stock_owner_first(), stock_dacl_first()] {
        let merged = LaunchSd::parse(&stock)
            .unwrap()
            .with_deny(&group())
            .unwrap();
        let read = LaunchSd::parse(&merged.to_bytes()).unwrap();
        let (rest, dropped) = read.without_denies(&group());
        assert_eq!(dropped, 1);
        assert_eq!(rest.to_bytes(), stock);
        assert!(!rest.names_deny(&group()));
        // Another group's deny and every allow stay.
        let other = Sid::parse("S-1-5-21-7-8-9-1010").unwrap();
        let mut theirs = read.clone();
        theirs.aces.push(entry(1, 0, 0x1F, &sid_bytes(&other)));
        let (rest, dropped) = theirs.without_denies(&group());
        assert_eq!((dropped, rest.aces.len()), (1, 4));
        assert!(rest.names_deny(&other));
    }
}

#[test]
fn a_sacl_and_an_odd_entry_are_kept_as_they_are() {
    let audit = acl(&[entry(2, 0xC0, 0x1F, &nt(11))]);
    // An object allow (type 5) holds no SID where the plain ones do: kept as bytes.
    let object = vec![5, 0, 12, 0, 0x1F, 0, 0, 0, 0, 0, 0, 0];
    let dacl = acl(&[object.clone(), entry(0, 0, 0xB, &nt(18))]);
    let bytes = laid_out(0x8014, &[(12, &audit), (16, &dacl), (4, &BA), (8, &BA)]);
    let sd = LaunchSd::parse(&bytes).unwrap();
    assert_eq!(sd.to_bytes(), bytes);
    let merged = sd.with_deny(&group()).unwrap();
    let again = LaunchSd::parse(&merged.to_bytes()).unwrap();
    assert_eq!(again.sacl.as_deref(), Some(audit.as_slice()));
    assert_eq!(again.aces[1], object);
    assert_eq!(again.without_denies(&group()).0.to_bytes(), bytes);
}

#[test]
fn the_built_in_default_is_the_vms_and_takes_the_deny() {
    // O:BAG:BAD:(A;;CCDCLCSWRP;;;BA)(A;;CCDCLCSWRP;;;IU)(A;;CCDCLCSWRP;;;SY)
    let dacl = acl(&[
        entry(0, 0, 0x1F, &BA),
        entry(0, 0, 0x1F, &nt(4)),
        entry(0, 0, 0x1F, &nt(18)),
    ]);
    let vm = laid_out(0x8004, &[(4, &BA), (8, &BA), (16, &dacl)]);
    let default = LaunchSd::built_in_default();
    assert_eq!(default.to_bytes(), vm);
    assert_eq!(LaunchSd::parse(&vm).unwrap(), default);
    let merged = default.with_deny(&group()).unwrap();
    assert!(merged.denies_first(&group()));
    assert_eq!(merged.without_denies(&group()).0, default);
    // Laid out the other way it is still the default.
    let other = laid_out(0x8004, &[(16, &dacl), (4, &BA), (8, &BA)]);
    assert_eq!(LaunchSd::parse(&other).unwrap(), default);
}

#[test]
fn malformed_values_are_refused() {
    let stock = stock_owner_first();
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("short", stock[..19].to_vec()),
        ("truncated", stock[..stock.len() - 4].to_vec()),
    ];
    let mut set = |name: &'static str, at: usize, bytes: &[u8]| {
        let mut odd = stock.clone();
        odd[at..at + bytes.len()].copy_from_slice(bytes);
        cases.push((name, odd));
    };
    set("revision 2", 0, &[2]);
    set("absolute", 2, &0x0004u16.to_le_bytes());
    set("no DACL", 2, &0x8000u16.to_le_bytes());
    set("NULL DACL", 16, &[0, 0, 0, 0]);
    set("no owner", 4, &[0, 0, 0, 0]);
    set("no group", 8, &[0, 0, 0, 0]);
    set("an offset inside the header", 4, &[8, 0, 0, 0]);
    set("an offset past the end", 16, &[0xFF, 0, 0, 0]);
    set("a SID of 16 parts", 0x14 + 1, &[16]);
    set("a SID of revision 2", 0x14, &[2]);
    set("an ACL revision 3", 0x34, &[3]);
    set("an ACL size past the end", 0x34 + 2, &[0x48]);
    set("an ACL of an odd size", 0x34 + 2, &[0x42]);
    set("one entry too many", 0x34 + 4, &[4]);
    set("an entry of size 4", 0x34 + 8 + 2, &[4]);
    set("an entry past its ACL", 0x34 + 8 + 2, &[0x40]);
    set("an entry's SID of 9 parts", 0x34 + 8 + 9, &[9]);
    for (name, bytes) in cases {
        assert!(LaunchSd::parse(&bytes).is_err(), "{name}");
    }
}

#[test]
fn the_resource_managers_control_byte_is_kept() {
    let mut bytes = stock_owner_first();
    bytes[1] = 0x5A;
    // SE_RM_CONTROL_VALID, which says the byte means something.
    bytes[3] |= 0x40;
    let sd = LaunchSd::parse(&bytes).unwrap();
    assert_eq!(sd.to_bytes(), bytes, "the same bytes back");
    let merged = sd.with_deny(&group()).unwrap();
    assert_eq!(merged.to_bytes()[1], 0x5A);
    assert_eq!(merged.without_denies(&group()).0.to_bytes(), bytes);
}
