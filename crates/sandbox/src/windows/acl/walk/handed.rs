//! A child's writes made, then read back (S15 `PR4h`): the hand-back's owner goes first, and a
//! hand-back is checked through the same handle ([`handed`]).

use std::io;

use windows::Win32::Security::{
    LABEL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
};

use super::Mode;
use crate::windows::acl::READ_ALL;
use crate::windows::acl::object::Object;
use crate::windows::acl_bytes::{
    Acl, Descriptor, Write, broad_allow, owner_bytes, owner_descriptor,
};
use crate::windows::writer::{ApplyFailure, Owned};

/// The writes `settle` decided on: the DACL (with how it is set), the hand-back's owner, the
/// label.
pub(super) struct Writes<'a> {
    pub(super) dacl: Option<(OBJECT_SECURITY_INFORMATION, &'a [u8])>,
    pub(super) owner: Option<&'a [u8]>,
    pub(super) label: Option<&'a [u8]>,
}

/// Makes `writes` on `child` in `order`, then reads it back when anything changed (or it was
/// `repaired`): a hand-back is checked there ([`handed`]).
pub(super) fn make(
    child: &Object,
    order: &[Write],
    writes: &Writes<'_>,
    repaired: bool,
    mode: Mode<'_>,
) -> Result<Option<Vec<u8>>, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    for write in order {
        match (write, writes.dacl, writes.owner, writes.label) {
            (Write::Dacl, Some((info, descriptor)), _, _) => child.set(info, descriptor),
            (Write::Owner, _, Some(me), _) => {
                child.set(OWNER_SECURITY_INFORMATION, &owner_descriptor(me))
            }
            (Write::Label, _, _, Some(descriptor)) => {
                child.set(LABEL_SECURITY_INFORMATION, descriptor)
            }
            _ => Ok(()),
        }
        .map_err(io)?;
    }
    let written = if !order.is_empty() || repaired {
        Some(child.security(READ_ALL).map_err(io)?)
    } else {
        None
    };
    if let (Mode::Reclaim { pool, me, own }, Some(_), Some(after)) = (mode, writes.owner, &written)
    {
        handed(after, pool, own, me)?;
    }
    Ok(written)
}

/// A hand-back read again through its handle once written: the owner is `me` and no explicit
/// allow for a pool SID but `own` (nor for a broad group) is left. Another pool user's allow put
/// back between the owner write and the DACL write, or an owner changed since, refuses as
/// [`ApplyFailure::PoolOwned`]. A handle that pool user opened with `WRITE_DAC` before the
/// owner write can still write later: that residual is ADR 0049's.
fn handed(after: &[u8], pool: &[Vec<u8>], own: &[Vec<u8>], me: &[u8]) -> Result<(), ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let others: Vec<Vec<u8>> = pool.iter().filter(|p| !own.contains(p)).cloned().collect();
    let dacl = Descriptor::parse(after).map_err(io)?.dacl;
    let dacl = dacl.map(Acl::parse).transpose().map_err(io)?;
    let allowed = dacl.as_ref().is_some_and(|d| broad_allow(d, &others));
    if owner_bytes(after) != Some(me) || allowed {
        return Err(ApplyFailure::PoolOwned {
            owned: Owned::Child,
        });
    }
    Ok(())
}
