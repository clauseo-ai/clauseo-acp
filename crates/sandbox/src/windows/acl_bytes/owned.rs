//! The protected DACL a folder Clauseo owns gets (`acl_plan::OwnedRoot`), worked out from the
//! one it has now: only explicit entries, none inherited.

use super::{ALLOWED, AceSpec, Acl, CONTAINER_INHERIT, INHERITED, OBJECT_INHERIT, render};
use super::{DENIED, flags, kind, sid};

/// `current` (an owned root's DACL now) as its protected DACL, in Windows' order:
///
/// 1. each explicit plain deny it has for one of `pool` (the plan's pool SIDs: the other pool
///    users' fence), as it is;
/// 2. an allow of `mask` (full access), inherited by files and folders inside (`OICI`), for each
///    of `full` (`SYSTEM`, maybe `Administrators`, the desktop user);
/// 3. each explicit plain allow it has for one of `allow` (the launch's own subjects: its pool
///    user's `Modify`), as it is, once.
///
/// Everything else goes: every inherited entry (`C:\`'s `Users` entries), and every other
/// explicit one (`Everyone`, `Users`, `CREATOR OWNER`, an object or callback entry). The plan's
/// own entries on the root are written again after, if one isn't kept.
pub(crate) fn owned_dacl(
    current: &Acl<'_>,
    mask: u32,
    full: &[Vec<u8>],
    allow: &[Vec<u8>],
    pool: &[Vec<u8>],
) -> Vec<u8> {
    let explicit = |ace: &&&[u8]| flags(ace) & INHERITED == 0;
    let named =
        |ace: &[u8], sids: &[Vec<u8>]| sid(ace).is_some_and(|s| sids.iter().any(|w| w == s));
    let fixed: Vec<Vec<u8>> = full
        .iter()
        .map(|sid| {
            let flags = OBJECT_INHERIT | CONTAINER_INHERIT;
            let (kind, sid) = (ALLOWED, sid.clone());
            AceSpec {
                kind,
                flags,
                mask,
                sid,
            }
            .bytes()
        })
        .collect();
    let mut out: Vec<&[u8]> = current
        .aces()
        .iter()
        .filter(explicit)
        .filter(|ace| kind(ace) == DENIED && named(ace, pool))
        .copied()
        .collect();
    out.extend(fixed.iter().map(Vec::as_slice));
    for ace in current.aces().iter().filter(explicit) {
        if kind(ace) == ALLOWED && named(ace, allow) && !out.contains(ace) {
            out.push(ace);
        }
    }
    render(2, &out)
}

#[cfg(test)]
mod tests {
    use super::super::{INHERIT_ONLY, sid_bytes};
    use super::*;
    use crate::windows::acl_plan::Sid;

    const POOL: &str = "S-1-5-21-1-2-3-1001";
    const OTHER: &str = "S-1-5-21-1-2-3-1002";
    const OWNER: &str = "S-1-5-21-7-8-9-1001";

    fn sid_of(text: &str) -> Vec<u8> {
        sid_bytes(&Sid::parse(text).unwrap())
    }

    fn ace(kind: u8, flags: u8, mask: u32, sid: &str) -> Vec<u8> {
        let sid = sid_of(sid);
        AceSpec {
            kind,
            flags,
            mask,
            sid,
        }
        .bytes()
    }

    fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
        let slices: Vec<&[u8]> = aces.iter().map(Vec::as_slice).collect();
        render(2, &slices)
    }

    /// The state folder under `C:\`, as the VM's observer found it: `C:\`'s `Users` entries
    /// inherited, the lawyer's and `SYSTEM`'s inherited, plus the plan's `Modify` and fence.
    #[test]
    fn only_the_fixed_entries_and_the_plans_own_stay() {
        let oici = OBJECT_INHERIT | CONTAINER_INHERIT;
        let modify = ace(ALLOWED, oici, 0x0013_01bf, POOL);
        let fence = ace(DENIED, oici, 0x000f_01ff, OTHER);
        let now = acl(&[
            fence.clone(),
            modify.clone(),
            ace(ALLOWED, 0, 0x1f_01ff, "S-1-5-32-545"),
            ace(ALLOWED, 0, 0x0012_00a9, OTHER),
            ace(DENIED, 0, 0x2, "S-1-1-0"),
            ace(ALLOWED, CONTAINER_INHERIT | INHERITED, 0x4, "S-1-5-32-545"),
            ace(ALLOWED, CONTAINER_INHERIT | INHERITED, 0x2, "S-1-5-32-545"),
            ace(
                ALLOWED,
                oici | INHERIT_ONLY | INHERITED,
                0x1000_0000,
                "S-1-3-0",
            ),
            ace(ALLOWED, oici | INHERITED, 0x1f_01ff, "S-1-5-18"),
        ]);
        let full = [sid_of("S-1-5-18"), sid_of(OWNER)];
        let pool = [sid_of(POOL), sid_of(OTHER)];
        let parsed = Acl::parse(&now).unwrap();
        let got = owned_dacl(&parsed, 0x1f_01ff, &full, &[sid_of(POOL)], &pool);
        let want = acl(&[
            fence,
            ace(ALLOWED, oici, 0x1f_01ff, "S-1-5-18"),
            ace(ALLOWED, oici, 0x1f_01ff, OWNER),
            modify,
        ]);
        assert_eq!(got, want);
        let again = owned_dacl(&Acl::parse(&got).unwrap(), 0x1f_01ff, &full, &[], &pool);
        let fixed_only = Acl::parse(&again).unwrap();
        assert_eq!(fixed_only.aces().len(), 3, "an allow not of `allow` goes");
    }

    #[test]
    fn a_dacl_already_protected_so_comes_back_the_same() {
        let oici = OBJECT_INHERIT | CONTAINER_INHERIT;
        let full = [sid_of("S-1-5-18"), sid_of(OWNER)];
        let now = acl(&[
            ace(ALLOWED, oici, 0x1f_01ff, "S-1-5-18"),
            ace(ALLOWED, oici, 0x1f_01ff, OWNER),
            ace(ALLOWED, oici, 0x0013_01bf, POOL),
        ]);
        let parsed = Acl::parse(&now).unwrap();
        let got = owned_dacl(&parsed, 0x1f_01ff, &full, &[sid_of(POOL)], &[]);
        assert_eq!(got, now);
    }
}
