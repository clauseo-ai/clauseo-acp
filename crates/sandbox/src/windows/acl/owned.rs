//! [`AclWriter::protect`](crate::windows::writer::AclWriter::protect) on NTFS: a folder Clauseo
//! owns gets its protected DACL through one handle, opened without following a link.

use std::io;

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION};

use super::open_planned;
use crate::windows::acl_bytes::{
    ALLOWED, AceSpec, Acl, CONTAINER_INHERIT, DACL_AUTO_INHERITED, DACL_PROTECTED, Descriptor,
    INHERIT_ONLY, OBJECT_INHERIT, build_descriptor, owned_dacl, sid_bytes,
};
use crate::windows::acl_plan::{Grant, OwnedRoot, Sid};
use crate::windows::record::current_user_sid;
use crate::windows::writer::{Added, ApplyFailure};

/// Gives `root` its protected DACL (see `AclWriter::protect`). A root missing by now is left
/// alone (`AlreadyThere`): NTFS holds nothing for it, and a folder made later inherits from its
/// own protected parent.
pub(super) fn protect(
    root: &OwnedRoot,
    pool: &[Sid],
    keep: &[Grant],
) -> Result<Added, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let Some(target) = open_planned(root.path())? else {
        return Ok(Added::AlreadyThere);
    };
    if !target.folder {
        return Err(ApplyFailure::Changed);
    }
    if target.is_volume_root().map_err(io)? {
        return Err(ApplyFailure::VolumeRoot);
    }
    let owner = current_user_sid().map_err(io)?;
    let fixed: Vec<AceSpec> = root
        .fixed(&owner)
        .iter()
        .map(|entry| {
            let only = if entry.inherit_only() {
                INHERIT_ONLY
            } else {
                0
            };
            AceSpec {
                kind: ALLOWED,
                flags: OBJECT_INHERIT | CONTAINER_INHERIT | only,
                mask: entry.mask(),
                sid: sid_bytes(entry.sid()),
            }
        })
        .collect();
    let keep: Vec<AceSpec> = keep
        .iter()
        .filter(|g| g.rights().is_allow())
        .map(|g| AceSpec::for_grant(g, true))
        .collect::<io::Result<_>>()
        .map_err(io)?;
    let pool: Vec<Vec<u8>> = pool.iter().map(sid_bytes).collect();
    let current = target.security(DACL_SECURITY_INFORMATION.0).map_err(io)?;
    let now = Descriptor::parse(&current).map_err(io)?;
    let dacl = Acl::parse(now.dacl.ok_or(ApplyFailure::NoDacl)?).map_err(io)?;
    let wanted = owned_dacl(&dacl, &fixed, &keep, &pool);
    if now.dacl_protected() && now.dacl == Some(wanted.as_slice()) {
        return Ok(Added::AlreadyThere);
    }
    if !target.write_dac {
        return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
    }
    let written = build_descriptor(DACL_PROTECTED | DACL_AUTO_INHERITED, Some(&wanted), None);
    let info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    target.set(info, &written).map_err(io)?;
    Ok(Added::Written)
}
