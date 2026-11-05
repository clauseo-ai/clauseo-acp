//! Descriptors rebuilt for the walk's repair of what an agent made (S15 PR3 review, M5): a new
//! owner, a cleaned DACL, the protection cleared, the rest kept.

use std::io;

use super::{DACL_PRESENT, Descriptor, SACL_PRESENT, SELF_RELATIVE, invalid, sid_at};

/// The self-relative descriptor `bytes` again, with `owner` as its owner when given, `dacl`
/// as its DACL when given, and the control bits `clear` cleared; its group and SACL kept. What
/// the walk hands `CreatePrivateObjectSecurityEx` after a repair, so `CREATOR OWNER` entries
/// still resolve to the owner.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the walk's repair is Windows only")
)]
pub(crate) fn rebuild(
    bytes: &[u8],
    owner: Option<&[u8]>,
    dacl: Option<&[u8]>,
    clear: u16,
) -> io::Result<Vec<u8>> {
    let now = Descriptor::parse(bytes)?;
    let owner = owner.or_else(|| sid_at(bytes, 4));
    let group = sid_at(bytes, 8);
    let dacl = dacl.or(now.dacl);
    let mut out = vec![1, 0, 0, 0];
    out.extend_from_slice(&[0; 16]);
    let mut control = (now.control & !clear & !(DACL_PRESENT | SACL_PRESENT)) | SELF_RELATIVE;
    let parts = [
        (owner, 4, 0),
        (group, 8, 0),
        (now.sacl, 12, SACL_PRESENT),
        (dacl, 16, DACL_PRESENT),
    ];
    for (part, offset_at, present) in parts {
        if let Some(part) = part {
            control |= present;
            let offset = u32::try_from(out.len()).map_err(invalid)?.to_le_bytes();
            if let Some(slot) = out.get_mut(offset_at..offset_at + 4) {
                slot.copy_from_slice(&offset);
            }
            out.extend_from_slice(part);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
    }
    if let Some(slot) = out.get_mut(2..4) {
        slot.copy_from_slice(&control.to_le_bytes());
    }
    Ok(out)
}

/// A self-relative descriptor holding only `owner`, for an owner write.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the owner writes are Windows only")
)]
pub(crate) fn owner_descriptor(owner: &[u8]) -> Vec<u8> {
    let mut out = vec![1, 0];
    out.extend_from_slice(&SELF_RELATIVE.to_le_bytes());
    out.extend_from_slice(&20_u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(owner);
    out
}
