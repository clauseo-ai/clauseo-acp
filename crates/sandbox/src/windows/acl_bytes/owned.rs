//! The protected DACL a folder Clauseo owns gets (`acl_plan::OwnedRoot`), worked out from the
//! one it has now: only explicit entries, none inherited.

use super::{ALLOWED, AceSpec, Acl, DENIED, INHERITED, flags, kind, render, sid};

/// `current` (an owned root's DACL now) as its protected DACL, in Windows' order:
///
/// 1. each explicit plain deny it has for one of `pool` (the plan's pool SIDs: the other pool
///    users' fence), as it is;
/// 2. `fixed`, the root's own entries (full access for `SYSTEM`, maybe `Administrators`, the
///    desktop user; `OWNER RIGHTS` on the agent's folders);
/// 3. each explicit allow it has that is exactly one of `keep` (the plan's allows there and
///    every allow the ledger records there for a launch, the same SID, mask and flags), once.
///
/// Everything else goes: every inherited entry (`C:\`'s `Users` entries), and every other
/// explicit one (`Everyone`, `Users`, `CREATOR OWNER`, an object or callback entry, an allow
/// whose mask isn't the one recorded). The plan's own entries are written again after, if one
/// isn't kept.
pub(crate) fn owned_dacl(
    current: &Acl<'_>,
    fixed: &[AceSpec],
    keep: &[AceSpec],
    pool: &[Vec<u8>],
) -> Vec<u8> {
    let explicit = |ace: &&&[u8]| flags(ace) & INHERITED == 0;
    let fixed: Vec<Vec<u8>> = fixed.iter().map(AceSpec::bytes).collect();
    let mut out: Vec<&[u8]> = current
        .aces()
        .iter()
        .filter(explicit)
        .filter(|ace| kind(ace) == DENIED && sid(ace).is_some_and(|s| pool.iter().any(|p| p == s)))
        .copied()
        .collect();
    out.extend(fixed.iter().map(Vec::as_slice));
    for ace in current.aces().iter().filter(explicit) {
        if kind(ace) == ALLOWED && keep.iter().any(|spec| spec.is(ace)) && !out.contains(ace) {
            out.push(ace);
        }
    }
    render(2, &out)
}

#[cfg(test)]
mod tests {
    use super::super::{CONTAINER_INHERIT, INHERIT_ONLY, OBJECT_INHERIT, sid_bytes};
    use super::*;
    use crate::windows::acl_plan::Sid;

    const POOL: &str = "S-1-5-21-1-2-3-1001";
    const OTHER: &str = "S-1-5-21-1-2-3-1002";
    const OWNER: &str = "S-1-5-21-7-8-9-1001";
    const OICI: u8 = OBJECT_INHERIT | CONTAINER_INHERIT;
    const MODIFY: u32 = 0x0013_01bf;
    const FULL: u32 = 0x001f_01ff;

    fn sid_of(text: &str) -> Vec<u8> {
        sid_bytes(&Sid::parse(text).unwrap())
    }

    fn spec(kind: u8, flags: u8, mask: u32, sid: &str) -> AceSpec {
        let sid = sid_of(sid);
        AceSpec {
            kind,
            flags,
            mask,
            sid,
        }
    }

    fn ace(kind: u8, flags: u8, mask: u32, sid: &str) -> Vec<u8> {
        spec(kind, flags, mask, sid).bytes()
    }

    fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
        let slices: Vec<&[u8]> = aces.iter().map(Vec::as_slice).collect();
        render(2, &slices)
    }

    /// `SYSTEM` and the desktop user, full access, and `OWNER RIGHTS`' inherit-only `Modify`.
    fn fixed() -> Vec<AceSpec> {
        vec![
            spec(ALLOWED, OICI, FULL, "S-1-5-18"),
            spec(ALLOWED, OICI, FULL, OWNER),
            spec(ALLOWED, OICI | INHERIT_ONLY, MODIFY, "S-1-3-4"),
        ]
    }

    /// The state folder under `C:\`, as the VM's observer found it: `C:\`'s `Users` entries
    /// inherited, the lawyer's and `SYSTEM`'s inherited, plus the plan's `Modify` and fence.
    #[test]
    fn only_the_fixed_entries_and_the_plans_own_stay() {
        let modify = ace(ALLOWED, OICI, MODIFY, POOL);
        let fence = ace(DENIED, OICI, 0x000f_01ff, OTHER);
        let now = acl(&[
            fence.clone(),
            modify.clone(),
            ace(ALLOWED, 0, FULL, "S-1-5-32-545"),
            ace(ALLOWED, 0, 0x0012_00a9, OTHER),
            ace(DENIED, 0, 0x2, "S-1-1-0"),
            ace(ALLOWED, CONTAINER_INHERIT | INHERITED, 0x4, "S-1-5-32-545"),
            ace(ALLOWED, CONTAINER_INHERIT | INHERITED, 0x2, "S-1-5-32-545"),
            ace(
                ALLOWED,
                OICI | INHERIT_ONLY | INHERITED,
                0x1000_0000,
                "S-1-3-0",
            ),
            ace(ALLOWED, OICI | INHERITED, FULL, "S-1-5-18"),
        ]);
        let pool = [sid_of(POOL), sid_of(OTHER)];
        let keep = [spec(ALLOWED, OICI, MODIFY, POOL)];
        let got = owned_dacl(&Acl::parse(&now).unwrap(), &fixed(), &keep, &pool);
        let mut want = vec![fence];
        want.extend(fixed().iter().map(AceSpec::bytes));
        want.push(modify);
        assert_eq!(got, acl(&want));
        let again = owned_dacl(&Acl::parse(&got).unwrap(), &fixed(), &[], &pool);
        assert_eq!(
            Acl::parse(&again).unwrap().aces().len(),
            4,
            "an allow not kept goes"
        );
    }

    /// The review's L3: another live session's allow the ledger records on a shared owned folder
    /// stays, as recorded; an allow for that SID with another mask than recorded doesn't.
    #[test]
    fn another_sessions_recorded_allow_stays_only_with_its_recorded_mask() {
        let theirs = ace(ALLOWED, OICI, MODIFY, OTHER);
        let widened = ace(ALLOWED, OICI, FULL, OTHER);
        let now = acl(&[theirs.clone(), widened]);
        let keep = [spec(ALLOWED, OICI, MODIFY, OTHER)];
        let got = owned_dacl(&Acl::parse(&now).unwrap(), &fixed(), &keep, &[]);
        let mut want: Vec<Vec<u8>> = fixed().iter().map(AceSpec::bytes).collect();
        want.push(theirs);
        assert_eq!(got, acl(&want));
    }

    #[test]
    fn a_dacl_already_protected_so_comes_back_the_same() {
        let mut aces: Vec<Vec<u8>> = fixed().iter().map(AceSpec::bytes).collect();
        aces.push(ace(ALLOWED, OICI, MODIFY, POOL));
        let now = acl(&aces);
        let keep = [spec(ALLOWED, OICI, MODIFY, POOL)];
        let got = owned_dacl(&Acl::parse(&now).unwrap(), &fixed(), &keep, &[]);
        assert_eq!(got, now);
    }
}
