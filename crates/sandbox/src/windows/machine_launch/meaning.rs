//! What a descriptor means, for comparing two by meaning rather than by bytes: its owner, its
//! group, and the set of its DACL's entries, each as type, flags, mask and SID. Order, layout and
//! padding don't count. DCOM's launcher service rewrites both values within a millisecond of a
//! write (a VM run): it merges Windows' default entries into the launch value, and re-serialises
//! the access value with padding. So the removal finds Clauseo's own bytes again only by
//! meaning. Read leniently (no COM rules on the entries), since only comparing reads it so;
//! nothing read here is written. Pure; tested on every OS.

use std::collections::BTreeSet;

use super::sd::{
    DACL_PRESENT, HEADER, SELF_RELATIVE, SdError, acl_at, flags, kind, mask, sid, sid_at, u16_at,
    u32_at,
};

/// One entry by meaning: type, flags, mask, then the SID (for the plain types) or the rest of
/// its bytes (for the others).
type Entry = (u8, u8, u32, Vec<u8>);

/// A descriptor's meaning: owner, group, and the set of its DACL's entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meaning {
    /// The owner's SID; `None` when there is none.
    owner: Option<Vec<u8>>,
    /// The group's SID; `None` when there is none.
    group: Option<Vec<u8>>,
    /// The DACL's entries, as a set.
    aces: BTreeSet<Entry>,
}

impl Meaning {
    /// The meaning of a descriptor whose parts are these.
    pub(super) fn from_parts(owner: Option<&[u8]>, group: Option<&[u8]>, aces: &[Vec<u8>]) -> Self {
        let entry = |bytes: &Vec<u8>| -> Entry {
            let rest = sid(bytes).or_else(|| bytes.get(8..)).unwrap_or_default();
            (
                kind(bytes),
                flags(bytes),
                mask(bytes).unwrap_or(0),
                rest.to_vec(),
            )
        };
        Meaning {
            owner: owner.map(<[u8]>::to_vec),
            group: group.map(<[u8]>::to_vec),
            aces: aces.iter().map(entry).collect(),
        }
    }

    /// The meaning of the self-relative descriptor `bytes`: a header, an owner and a group when
    /// set, and a DACL, present and not NULL.
    pub fn of(bytes: &[u8]) -> Result<Meaning, SdError> {
        if bytes.len() < HEADER || bytes.first() != Some(&1) {
            return Err(SdError("shorter than its header, or not revision 1"));
        }
        let control = u16_at(bytes, 2).ok_or(SdError("no control word"))?;
        if control & SELF_RELATIVE == 0 {
            return Err(SdError("not self-relative"));
        }
        let offset = |at: usize| -> Result<usize, SdError> {
            let offset = u32_at(bytes, at)
                .and_then(|raw| usize::try_from(raw).ok())
                .ok_or(SdError("no offset"))?;
            if offset != 0 && offset < HEADER {
                return Err(SdError("an offset inside the header"));
            }
            Ok(offset)
        };
        let part = |at: usize| -> Result<Option<&[u8]>, SdError> {
            match offset(at)? {
                0 => Ok(None),
                at => sid_at(bytes, at).map(Some),
            }
        };
        let (owner, group, dacl_at) = (part(4)?, part(8)?, offset(16)?);
        if control & DACL_PRESENT == 0 || dacl_at == 0 {
            return Err(SdError("no DACL, or a NULL one"));
        }
        let (_, aces, _) = acl_at(bytes, dacl_at)?;
        Ok(Meaning::from_parts(owner, group, &aces))
    }

    /// Whether `a` and `b` are descriptors that mean the same; `false` when either won't read.
    pub fn same(a: &[u8], b: &[u8]) -> bool {
        matches!((Meaning::of(a), Meaning::of(b)), (Ok(a), Ok(b)) if a == b)
    }
}
