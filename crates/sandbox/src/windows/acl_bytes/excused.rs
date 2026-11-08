//! The strict walk's one exception (R18): a read of another matter is an explicit allow for a
//! pool user that other matter's fence denies, on a child of its tree, which is exactly what
//! [`broad_allow`](super::broad_allow) finds. The ledger lists each one; the walk excuses an
//! allow only when it is that read, on the path the ledger lists it on, for the SID it names:
//! a plain allow of exactly `ReadAndExecute`, explicit, for the item alone or its tree. Any other
//! allow there, or one the agent wrote elsewhere, still refuses or is repaired.

use super::{ALLOWED, Acl, CONTAINER_INHERIT, OBJECT_INHERIT, broad_sids, flags, kind, mask};
use super::{offends, render, sid};
use crate::windows::acl_plan::Rights;

/// Whether `ace` is a read of another matter's allow for one of `here`, the SIDs the ledger
/// lists a read for on the item.
fn excused(ace: &[u8], here: &[&[u8]]) -> bool {
    let whole = OBJECT_INHERIT | CONTAINER_INHERIT;
    kind(ace) == ALLOWED
        && mask(ace) == Some(Rights::ReadAndExecute.mask())
        && (flags(ace) == 0 || flags(ace) == whole)
        && sid(ace).is_some_and(|s| here.contains(&s))
}

/// [`broad_allow`](super::broad_allow), but an allow the ledger lists as a read of another
/// matter on the item, for one of `here`, doesn't count.
pub(crate) fn broad_allow_but(dacl: &Acl<'_>, watched: &[Vec<u8>], here: &[&[u8]]) -> bool {
    let broad = broad_sids(watched);
    let aces = dacl.aces().iter();
    aces.copied()
        .any(|ace| offends(ace, &broad) && !excused(ace, here))
}

/// [`without_broad`](super::without_broad), keeping each read of another matter for one of
/// `here`.
pub(crate) fn without_broad_but(dacl: &Acl<'_>, watched: &[Vec<u8>], here: &[&[u8]]) -> Vec<u8> {
    let broad = broad_sids(watched);
    let kept: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|ace| !offends(ace, &broad) || excused(ace, here))
        .collect();
    render(dacl.revision.max(2), &kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::acl_bytes::{AceSpec, empty_acl, sid_bytes};
    use crate::windows::acl_plan::{Grant, Inherit};
    use crate::windows::admit::PathKind;
    use crate::windows::sids::Sid;

    const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

    fn allow(rights: Rights, inherit: Inherit) -> AceSpec {
        let grant = Grant {
            path: "C:\\m2\\docs".into(),
            sid: Sid::parse(POOL).unwrap(),
            rights,
            inherit,
            kind: PathKind::OtherMatterRead,
        };
        AceSpec::for_grant(&grant, true).unwrap()
    }

    #[test]
    fn only_the_listed_read_on_its_item_is_excused() {
        let pool = sid_bytes(&Sid::parse(POOL).unwrap());
        let watched = [pool.clone()];
        let empty = empty_acl();
        for inherit in [Inherit::ThisOnly, Inherit::Tree] {
            let with = Acl::parse(&empty)
                .unwrap()
                .with(&allow(Rights::ReadAndExecute, inherit));
            let dacl = Acl::parse(&with).unwrap();
            assert!(super::super::broad_allow(&dacl, &watched));
            assert!(!broad_allow_but(&dacl, &watched, &[&pool]));
            assert!(broad_allow_but(&dacl, &watched, &[]), "not listed here");
            let kept = without_broad_but(&dacl, &watched, &[&pool]);
            assert_eq!(Acl::parse(&kept).unwrap().aces().len(), 1);
        }
        // Any other right for the same SID still counts, and is repaired away.
        let with = Acl::parse(&empty)
            .unwrap()
            .with(&allow(Rights::Modify, Inherit::Tree));
        let dacl = Acl::parse(&with).unwrap();
        assert!(broad_allow_but(&dacl, &watched, &[&pool]));
        let kept = without_broad_but(&dacl, &watched, &[&pool]);
        assert_eq!(Acl::parse(&kept).unwrap().aces().len(), 0);
    }
}
