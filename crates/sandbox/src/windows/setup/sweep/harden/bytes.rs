//! The byte work of hardening (`harden.rs`): a task's self-relative descriptor read, its
//! DACL's loose allows tightened, and the descriptor laid out again with every other part as
//! it was. Pure, on `acl_bytes`: tested on every OS.

use std::fmt::Display;

use super::{Failed, Loosened, STRIPPED, TASK_WRITE, Tightened};
use crate::windows::acl_bytes::{
    Acl, Descriptor, kind, mask, owner_bytes, owner_sid, sid, sid_text,
};
use crate::windows::sids::Sid;

/// The owner of the self-relative descriptor `descriptor`, when it names one.
pub(super) fn owner_of(descriptor: &[u8]) -> Option<Sid> {
    owner_sid(descriptor)
}

/// The self-relative descriptor `descriptor` with each allow that gives one of [`TASK_WRITE`] to
/// a SID `loose` holds tightened, as the module says: [`STRIPPED`] taken from its mask (the
/// entry otherwise byte for byte, its flags kept), or the entry removed when nothing is left.
/// `Ok(None)` when no allow is; `Err` when the descriptor or its DACL doesn't parse, or it has
/// no DACL (a NULL one: everyone may write, and only an added DACL would close it).
pub fn tightened(
    descriptor: &[u8],
    loose: &mut dyn FnMut(Option<&Sid>) -> bool,
) -> Result<Tightened, Failed> {
    let malformed =
        |why: &dyn Display| format!("the task's security descriptor can't be read ({why})");
    let parsed = Descriptor::parse(descriptor).map_err(|error| malformed(&error))?;
    let Some(dacl) = parsed.dacl else {
        return Err(
            "the task has a NULL DACL, which hardening can't close without adding to \
                    it"
            .to_owned(),
        );
    };
    let acl = Acl::parse(dacl).map_err(|error| malformed(&error))?;
    let mut kept: Vec<Vec<u8>> = Vec::with_capacity(acl.aces().len());
    let mut changes = Vec::new();
    for ace in acl.aces() {
        let rights = mask(ace).filter(|_| matches!(kind(ace), 0 | 4 | 5 | 9 | 11));
        let Some(rights) = rights.filter(|rights| rights & TASK_WRITE != 0) else {
            kept.push(ace.to_vec());
            continue;
        };
        let named = sid(ace).and_then(sid_text);
        if !loose(named.as_ref()) {
            kept.push(ace.to_vec());
            continue;
        }
        let left = rights & !STRIPPED;
        changes.push(Loosened {
            sid: named,
            lost: rights & STRIPPED,
            removed: left == 0,
        });
        if left != 0 {
            let mut ace = ace.to_vec();
            if let Some(slot) = ace.get_mut(4..8) {
                slot.copy_from_slice(&left.to_le_bytes());
            }
            kept.push(ace);
        }
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let dacl = with_aces(dacl, &kept);
    let rebuilt = with_dacl(descriptor, parsed.sacl, &dacl).ok_or_else(|| malformed(&"parts"))?;
    Ok(Some((rebuilt, changes)))
}

/// The ACL `acl` with `aces` instead of its entries: its revision and reserved bytes kept.
fn with_aces(acl: &[u8], aces: &[Vec<u8>]) -> Vec<u8> {
    let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(acl.get(..2).unwrap_or(&[2, 0]));
    out.extend_from_slice(&u16::try_from(size).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&u16::try_from(aces.len()).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(acl.get(6..8).unwrap_or(&[0, 0]));
    for ace in aces {
        out.extend_from_slice(ace);
    }
    out
}

/// The self-relative descriptor `descriptor` with `dacl` as its DACL: its revision, control,
/// owner, group and `sacl` as they are. `None` when its group can't be read.
fn with_dacl(descriptor: &[u8], sacl: Option<&[u8]>, dacl: &[u8]) -> Option<Vec<u8>> {
    let group = group_bytes(descriptor).ok()?;
    let mut out = descriptor.get(..4)?.to_vec();
    out.extend_from_slice(&[0; 16]);
    let parts = [
        (owner_bytes(descriptor), 4),
        (group, 8),
        (sacl, 12),
        (Some(dacl), 16),
    ];
    for (part, offset_at) in parts {
        if let Some(part) = part {
            let offset = u32::try_from(out.len()).ok()?.to_le_bytes();
            out.get_mut(offset_at..offset_at + 4)?
                .copy_from_slice(&offset);
            out.extend_from_slice(part);
        }
    }
    Some(out)
}

/// The group of the self-relative descriptor `descriptor`, as bytes: `Ok(None)` when it
/// names none, `Err` when its offset runs past it.
fn group_bytes(descriptor: &[u8]) -> Result<Option<&[u8]>, &'static str> {
    let past = "the descriptor's group runs past it";
    let offset = descriptor.get(8..12).ok_or(past)?;
    let offset = u32::from_le_bytes(offset.try_into().ok().ok_or(past)?);
    if offset == 0 {
        return Ok(None);
    }
    let at = usize::try_from(offset).ok().ok_or(past)?;
    let at = descriptor.get(at..).ok_or(past)?;
    let len = 8 + 4 * usize::from(*at.get(1).ok_or(past)?);
    at.get(..len).map(Some).ok_or(past)
}

/// Whether `back`, a task's descriptor read back after hardening wrote it, keeps the owner,
/// group and SACL of `read`, the one hardening read before: `Err` naming each that differs
/// or can't be read. Compared as bytes, wherever each sits in either.
pub(super) fn kept_as_read(read: &[u8], back: &[u8]) -> Result<(), String> {
    let mut differ = Vec::new();
    if owner_bytes(read).is_none() || owner_bytes(read) != owner_bytes(back) {
        differ.push("owner");
    }
    if group_bytes(read).is_err() || group_bytes(read) != group_bytes(back) {
        differ.push("group");
    }
    if sacl_bytes(read).is_err() || sacl_bytes(read) != sacl_bytes(back) {
        differ.push("SACL");
    }
    if differ.is_empty() {
        Ok(())
    } else {
        Err(format!("its {} read back changed", differ.join(", ")))
    }
}

/// The SACL of the self-relative descriptor `descriptor`: `Ok(None)` when it has none, `Err`
/// when it doesn't parse.
fn sacl_bytes(descriptor: &[u8]) -> Result<Option<&[u8]>, &'static str> {
    let parsed = Descriptor::parse(descriptor)
        .ok()
        .ok_or("the descriptor doesn't parse")?;
    Ok(parsed.sacl)
}
