//! `--reclaim`'s objects on NTFS (S15 `PR4i`): [`NtfsOwners`], the Windows side of
//! `reclaim::walk`'s [`OwnerTree`]. Opened as the permission walk opens them (`object.rs`): the
//! root by its path, never through a link, its final path checked and never a volume's root;
//! each child by its name relative to its folder's handle (`OBJ_DONT_REPARSE`), its final path
//! its folder's plus `\name`, and each folder's final path checked again before it is listed.
//! Run with `SeBackupPrivilege` and `SeRestorePrivilege` enabled (`privilege.rs`), the opens get
//! `READ_CONTROL`, the listing, `WRITE_DAC` and `WRITE_OWNER` whatever the DACL says, as a
//! backup-intent open does. The writes, on that handle: the owner (`OWNER_SECURITY_INFORMATION`
//! alone), read back after; and just before it, when the DACL holds an `OWNER RIGHTS` entry, the
//! DACL without those entries and with every other byte as it was (`without_owner_rights`), read
//! back after too. Each object re-owned is kept by its final path and file ID, for the result.
//! Paths are resolved by handle (`resolve`), so a short name, a different case or a
//! junction on the way is checked as the final path it reaches.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

use super::object::{Child, Object, reparse_refused};
use super::{READ_ALL, dacl_info, same_path};
use crate::windows::acl_bytes::{
    Descriptor, owner_descriptor, owner_sid, sid_bytes, without_owner_rights,
};
use crate::windows::reclaim::walk::{Kind, OwnerTree};
use crate::windows::sids::Sid;
use crate::windows::writer::{ApplyFailure, FileId};

/// What the desktop user's own token gets on a descriptor (`Owner::granted`).
pub(in crate::windows) type Granted<'a> = &'a dyn Fn(&[u8]) -> io::Result<u32>;

/// The NTFS objects `--reclaim` walks, with `granted` saying what the desktop user's own token
/// gets on each; and each object it gave the owner, by its final path and file ID.
pub(in crate::windows) struct NtfsOwners<'a> {
    granted: Granted<'a>,
    taken: Vec<(PathBuf, FileId)>,
}

impl<'a> NtfsOwners<'a> {
    pub(in crate::windows) fn new(granted: Granted<'a>) -> NtfsOwners<'a> {
        NtfsOwners {
            granted,
            taken: Vec::new(),
        }
    }

    /// The objects given the owner since the last call, by final path and file ID: what the
    /// result reports, so the hand-back strips denies on those alone. One whose file ID couldn't
    /// be read isn't listed.
    pub(in crate::windows) fn drain_taken(&mut self) -> Vec<(PathBuf, FileId)> {
        std::mem::take(&mut self.taken)
    }
}

/// An object opened as itself, and its final path.
#[derive(Debug)]
pub(in crate::windows) struct Node {
    object: Object,
    path: Vec<u16>,
}

impl OwnerTree for NtfsOwners<'_> {
    type Node = Node;
    type Name = Child;

    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf> {
        let object = Object::open(path)?;
        Ok(PathBuf::from(OsString::from_wide(&object.normalized()?)))
    }

    fn open_root(&mut self, root: &Path) -> io::Result<Node> {
        let object = match Object::open_folder(root) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Object::open(root)?,
            other => other?,
        };
        if object.is_volume_root()? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the clean-up never walks a volume's root",
            ));
        }
        if !object.name_surrogate() && !same_path(&object, root)? {
            return Err(ApplyFailure::Changed.into());
        }
        let path = object.normalized()?;
        Ok(Node { object, path })
    }

    fn names(&mut self, folder: &Node) -> io::Result<Vec<Child>> {
        // A folder moved since it was checked is listed no more.
        if folder.object.normalized()? != folder.path {
            return Err(ApplyFailure::Changed.into());
        }
        folder.object.children()
    }

    fn open(&mut self, folder: &Node, name: &Child) -> io::Result<Option<Node>> {
        // A folder opened to be listed; one the open can't list (no Backup, as in a test) is
        // opened without it, so its owner and rights are still read, and the walk won't enter.
        let opened = match folder.object.open_child(&name.name, name.folder) {
            Err(error) if name.folder && error.kind() == io::ErrorKind::PermissionDenied => {
                folder.object.open_child(&name.name, false)
            }
            other => other,
        };
        let object = match opened {
            Ok(object) => object,
            Err(error) if reparse_refused(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut path = folder.path.clone();
        path.push(u16::from(b'\\'));
        path.extend_from_slice(&name.name);
        if !object.name_surrogate() && object.normalized()? != path {
            return Err(ApplyFailure::Changed.into());
        }
        Ok(Some(Node { object, path }))
    }

    fn kind(&self, node: &Node) -> Kind {
        match &node.object {
            object if object.name_surrogate() => Kind::Link,
            object if object.folder => Kind::Folder,
            object if object.links > 1 => Kind::Linked,
            _ => Kind::File,
        }
    }

    fn owner(&mut self, node: &Node) -> io::Result<Sid> {
        let bytes = node.object.security(OWNER_SECURITY_INFORMATION.0)?;
        owner_sid(&bytes).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no owner"))
    }

    fn granted(&mut self, node: &Node) -> io::Result<u32> {
        let descriptor = node.object.security(READ_ALL)?;
        (self.granted)(&descriptor)
    }

    fn drop_owner_rights(&mut self, node: &Node) -> io::Result<()> {
        let current = node.object.security(DACL_SECURITY_INFORMATION.0)?;
        let Some(new) = without_owner_rights(&current)? else {
            return Ok(());
        };
        if !node.object.write_dac {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the clean-up's handle got no WRITE_DAC",
            ));
        }
        let control = Descriptor::parse(&new)?.control;
        node.object.set(dacl_info(control), &new)?;
        let after = node.object.security(DACL_SECURITY_INFORMATION.0)?;
        if without_owner_rights(&after)?.is_some() {
            return Err(io::Error::other("an OWNER RIGHTS entry is still there"));
        }
        Ok(())
    }

    fn set_owner(&mut self, node: &Node, owner: &Sid) -> io::Result<()> {
        if !node.object.write_owner {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the clean-up's handle got no WRITE_OWNER",
            ));
        }
        node.object.set(
            OWNER_SECURITY_INFORMATION,
            &owner_descriptor(&sid_bytes(owner)),
        )?;
        if self.owner(node)? != *owner {
            return Err(io::Error::other(
                "the owner read back isn't the one written",
            ));
        }
        if let Ok((volume, id)) = node.object.file_id() {
            let path = PathBuf::from(OsString::from_wide(&node.path));
            self.taken.push((path, FileId { volume, id }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
