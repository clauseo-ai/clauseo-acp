//! The strict walk's one exception (R18): a read of another matter is an explicit allow for a
//! pool user that other matter's fence denies, on a child of its tree, which is exactly what
//! [`broad_allow`](super::broad_allow) finds. The ledger lists each one; the walk excuses an
//! allow only when it is that read, on the path the ledger lists it on, for the SID it names,
//! reaching as far as the entry says: a plain allow of exactly `ReadAndExecute`, explicit, for
//! the item alone where the entry is for the item alone (a matter folder's root, a folder
//! holding a refused name, a file), for its tree where the entry is for its tree. Any other
//! allow there (an inheriting one where the ledger lists a read of the item alone, say), or one
//! the agent wrote elsewhere, still refuses or is repaired.

use super::{ALLOWED, Acl, CONTAINER_INHERIT, OBJECT_INHERIT, broad_sids, flags, kind, mask};
use super::{offends, render, sid};
use crate::windows::acl_plan::{Inherit, Rights};

/// A read of another matter the ledger lists on an item: the SID its allow names, as bytes, and
/// the flags the allow is written with ([`read_flags`]).
pub(crate) type Listed<'r> = (&'r [u8], u8);

/// The flags a read of another matter listed with `inherit` is written with: none for the item
/// alone, object and container inherit for a folder's tree (a read's tree is always a folder's).
/// `None` for a shape no read takes, which excuses nothing.
pub(crate) fn read_flags(inherit: Inherit) -> Option<u8> {
    match inherit {
        Inherit::ThisOnly => Some(0),
        Inherit::Tree => Some(OBJECT_INHERIT | CONTAINER_INHERIT),
        Inherit::FilesInFolder | Inherit::Below => None,
    }
}

/// Whether `ace` is a read of another matter's allow listed in `here` for the item: its SID,
/// and exactly the flags that read is written with.
fn excused(ace: &[u8], here: &[Listed<'_>]) -> bool {
    kind(ace) == ALLOWED
        && mask(ace) == Some(Rights::ReadAndExecute.mask())
        && sid(ace).is_some_and(|s| here.contains(&(s, flags(ace))))
}

/// [`broad_allow`](super::broad_allow), but an allow the ledger lists as a read of another
/// matter on the item, for one of `here`, doesn't count.
pub(crate) fn broad_allow_but(dacl: &Acl<'_>, watched: &[Vec<u8>], here: &[Listed<'_>]) -> bool {
    let broad = broad_sids(watched);
    let aces = dacl.aces().iter();
    aces.copied()
        .any(|ace| offends(ace, &broad) && !excused(ace, here))
}

/// [`without_broad`](super::without_broad), keeping each read of another matter for one of
/// `here`.
pub(crate) fn without_broad_but(
    dacl: &Acl<'_>,
    watched: &[Vec<u8>],
    here: &[Listed<'_>],
) -> Vec<u8> {
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
    use crate::windows::acl_plan::Grant;
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
            let listed = [(pool.as_slice(), read_flags(inherit).unwrap())];
            let with = Acl::parse(&empty)
                .unwrap()
                .with(&allow(Rights::ReadAndExecute, inherit));
            let dacl = Acl::parse(&with).unwrap();
            assert!(super::super::broad_allow(&dacl, &watched));
            assert!(!broad_allow_but(&dacl, &watched, &listed));
            assert!(broad_allow_but(&dacl, &watched, &[]), "not listed here");
            let kept = without_broad_but(&dacl, &watched, &listed);
            assert_eq!(Acl::parse(&kept).unwrap().aces().len(), 1);
        }
        // Any other right for the same SID still counts, and is repaired away.
        let with = Acl::parse(&empty)
            .unwrap()
            .with(&allow(Rights::Modify, Inherit::Tree));
        let dacl = Acl::parse(&with).unwrap();
        let listed = [(pool.as_slice(), read_flags(Inherit::Tree).unwrap())];
        assert!(broad_allow_but(&dacl, &watched, &listed));
        let kept = without_broad_but(&dacl, &watched, &listed);
        assert_eq!(Acl::parse(&kept).unwrap().aces().len(), 0);
    }

    #[test]
    fn a_read_is_excused_only_in_the_shape_the_ledger_lists_it() {
        let pool = sid_bytes(&Sid::parse(POOL).unwrap());
        let watched = [pool.clone()];
        let empty = empty_acl();
        let shapes = [
            (Inherit::ThisOnly, Inherit::Tree),
            (Inherit::Tree, Inherit::ThisOnly),
        ];
        for (listed, written) in shapes {
            let here = [(pool.as_slice(), read_flags(listed).unwrap())];
            let with = Acl::parse(&empty)
                .unwrap()
                .with(&allow(Rights::ReadAndExecute, written));
            let dacl = Acl::parse(&with).unwrap();
            // An inheriting allow where the root's lift or a refused name's folder lists a read
            // of the item alone would reach every child: it refuses, and is repaired away.
            assert!(
                broad_allow_but(&dacl, &watched, &here),
                "{written:?} as {listed:?}"
            );
            let kept = without_broad_but(&dacl, &watched, &here);
            assert_eq!(Acl::parse(&kept).unwrap().aces().len(), 0);
        }
        // No read is listed for what is below a folder alone.
        assert_eq!(read_flags(Inherit::Below), None);
        assert_eq!(read_flags(Inherit::FilesInFolder), None);
    }
}
