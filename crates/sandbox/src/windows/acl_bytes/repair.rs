//! Descriptors rebuilt for the walk's repair of what an agent made (S15 PR3 review, M5): a new
//! owner, a cleaned DACL, the protection cleared, the rest kept.

use std::io;

#[cfg(any(windows, test))]
use super::copies::is_copy;
use super::{
    Acl, DACL_AUTO_INHERITED, DACL_PRESENT, Descriptor, INHERITED, SACL_PRESENT, SELF_RELATIVE,
    flags, invalid, render, sid_at,
};

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

/// `descriptor` with its DACL's inherited entries dropped; the DACL marked auto-inherited, the
/// owner, group, SACL and other control bits kept. When the DACL is not marked auto-inherited,
/// every explicit entry that copies an inherited one goes too ([`is_copy`]: equal but for the
/// inherited flag to one of `due`'s inherited entries). `due` is the
/// ACL the parent alone gives a new child of this kind and owner. The walk hands
/// `CreatePrivateObjectSecurityEx` this for every child not protected, so its result holds the
/// parent's entries once, all inherited.
///
/// Under a parent that isn't auto-inherited, NTFS writes a child's inherited copies without the
/// inherited flag (flags `00` in the kernel's DACL; `icacls` and `Get-Acl` still show `(I)`):
/// kept, they stay explicit, so a `Users` allow refused a strict walk (the Windows run 3, row
/// 15, a plain `C:\m`) and a plain walk left each entry twice, explicit and inherited. Such a
/// child's DACL is never marked auto-inherited. On an auto-inherited child an explicit entry is
/// one somebody wrote, ours or the lawyer's, and stays: folded into an inherited one it would go
/// with a later change to the parent (round 9). A descriptor with no DACL comes back as it is.
#[cfg(any(windows, test))]
pub(crate) fn without_inherited(descriptor: &[u8], due: &[u8]) -> io::Result<Vec<u8>> {
    let now = Descriptor::parse(descriptor)?;
    let Some(dacl) = now.dacl else {
        return Ok(descriptor.to_vec());
    };
    let plain = now.control & DACL_AUTO_INHERITED == 0;
    Acl::parse(dacl)?;
    let due = Acl::parse(due)?;
    keeping(descriptor, |ace| {
        flags(ace) & INHERITED == 0 && !(plain && is_copy(ace, &due))
    })
}

/// The descriptor a new child with `descriptor`'s owner and group is worked out from: an empty
/// DACL marked auto-inherited. What its parent alone gives such a child is what
/// `CreatePrivateObjectSecurityEx` makes of it, with `CREATOR OWNER` resolved to its owner.
#[cfg(any(windows, test))]
pub(crate) fn bare_child(descriptor: &[u8]) -> io::Result<Vec<u8>> {
    keeping(descriptor, |_| false)
}

/// `descriptor` with only the DACL entries `keep` takes, marked auto-inherited; the owner, group,
/// SACL and other control bits kept. A descriptor with no DACL comes back as it is.
#[cfg(any(windows, test))]
fn keeping(descriptor: &[u8], keep: impl Fn(&[u8]) -> bool) -> io::Result<Vec<u8>> {
    let Some(dacl) = Descriptor::parse(descriptor)?.dacl else {
        return Ok(descriptor.to_vec());
    };
    let dacl = Acl::parse(dacl)?;
    let kept: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|ace| keep(ace))
        .collect();
    let kept = render(dacl.revision.max(2), &kept);
    let mut out = rebuild(descriptor, None, Some(&kept), 0)?;
    let control = Descriptor::parse(&out)?.control | DACL_AUTO_INHERITED;
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
