//! Descriptors for the tests, built entry by entry, and the SIDs they name.

use super::sd::sid_bytes;
use crate::windows::sids::Sid;

/// `ClauseoSandboxUsers` on the tests' machine.
pub(super) const GROUP: &str = "S-1-5-21-1111111111-2222222222-3333333333-1010";
/// A pool user.
pub(super) const POOL_USER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1011";
/// The lawyer.
pub(super) const LAWYER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
/// Another group on the machine.
pub(super) const OTHER_GROUP: &str = "S-1-5-21-1111111111-2222222222-3333333333-1020";
/// The `AppContainer` capability Windows 11's value grants (`S-1-15-3-1024-...`).
const CAPABILITY: &str = concat!(
    "S-1-15-3-1024-2405443489-874036122-4286035555-1823921565-",
    "1746547431-2453885448-3625952902-991631256"
);

/// `ACCESS_ALLOWED_ACE_TYPE`.
pub(super) const A: u8 = 0;
/// `ACCESS_DENIED_ACE_TYPE`.
pub(super) const D: u8 = 1;

/// `text` as a [`Sid`].
pub(super) fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// One entry: type, flags, mask, SID.
pub(super) fn ace(kind: u8, flags: u8, mask: u32, who: &str) -> Vec<u8> {
    let who = sid_bytes(&sid(who));
    let mut out = vec![kind, flags];
    out.extend_from_slice(&u16::try_from(8 + who.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(&who);
    out
}

/// A revision-2 ACL of `aces`.
pub(super) fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
    let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
    let mut out = vec![2, 0];
    out.extend_from_slice(&u16::try_from(size).unwrap().to_le_bytes());
    out.extend_from_slice(&u16::try_from(aces.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for entry in aces {
        out.extend_from_slice(entry);
    }
    out
}

/// A self-relative descriptor owned by Administrators, group Administrators, with the DACL
/// `aces`: owner, group, then the DACL when `dacl_first` is false, else the DACL first.
pub(super) fn descriptor(aces: &[Vec<u8>], dacl_first: bool) -> Vec<u8> {
    let admins = sid_bytes(&sid("S-1-5-32-544"));
    let dacl = acl(aces);
    let mut out = vec![1, 0, 0x04, 0x80];
    out.extend_from_slice(&[0; 16]);
    let parts: [(&[u8], usize); 3] = if dacl_first {
        [(&dacl, 16), (&admins, 4), (&admins, 8)]
    } else {
        [(&admins, 4), (&admins, 8), (&dacl, 16)]
    };
    for (bytes, slot) in parts {
        let at = u32::try_from(out.len()).unwrap().to_le_bytes();
        out[slot..slot + 4].copy_from_slice(&at);
        out.extend_from_slice(bytes);
    }
    out
}

/// Windows 11's own value's entries: `(A;;CCDCLCSWRP;;;BA)(A;;CCDCLCSW;;;WD)
/// (A;;CCDCLCSWRP;;;S-1-5-32-562)(A;;CCDCLCSWRP;;;LU)(A;;CCDCSW;;;AC)(A;;CCDCSW;;;<capability>)`.
pub(super) fn windows_11_aces() -> Vec<Vec<u8>> {
    vec![
        ace(A, 0, 0x1F, "S-1-5-32-544"),
        ace(A, 0, 0xF, "S-1-1-0"),
        ace(A, 0, 0x1F, "S-1-5-32-562"),
        ace(A, 0, 0x1F, "S-1-5-32-559"),
        ace(A, 0, 0xB, "S-1-15-2-1"),
        ace(A, 0, 0xB, CAPABILITY),
    ]
}

/// Windows 11's own value, as a self-relative descriptor laid out owner first.
pub(super) fn windows_11() -> Vec<u8> {
    descriptor(&windows_11_aces(), false)
}

/// The groups a token of `user` carries in an interactive logon, `user` first.
pub(super) fn token(user: &str, groups: &[&str]) -> Vec<Sid> {
    let mut sids = vec![sid(user)];
    // Everyone, Users, INTERACTIVE, Authenticated Users, This Organization, Local account.
    let usual = [
        "S-1-1-0",
        "S-1-5-32-545",
        "S-1-5-4",
        "S-1-5-11",
        "S-1-5-15",
        "S-1-5-113",
    ];
    sids.extend(usual.iter().chain(groups).map(|text| sid(text)));
    sids
}
