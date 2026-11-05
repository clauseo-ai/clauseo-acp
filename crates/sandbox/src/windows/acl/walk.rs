// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/acl.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md
//! The walk (S15 PR3): each file and folder below a root gets its inherited entries and Low
//! label worked out again from its parent's, and written if they changed.
//!
//! Nothing in it is looked up through a path but the root, whose final path must be the one
//! planned. Each folder is opened once, its handle kept, and listed through that handle; each
//! child is opened by its name relative to that handle, never following a link, and its final
//! path must be its folder's plus `\name` before anything is written on it. Before a folder is
//! listed, its final path must still be the one checked. So a junction swapped in for a
//! subfolder while the walk runs is opened itself (and skipped), never passed through, and a
//! folder moved away after its check refuses the walk (`Changed`). So does a root that is a
//! junction now, and in a strict walk a child whose open met a reparse point (a plain walk skips
//! it).

#[cfg(test)]
use std::ffi::OsString;
use std::io;
#[cfg(test)]
use std::os::windows::ffi::OsStringExt as _;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use windows::Win32::Security::{
    CreatePrivateObjectSecurityEx, DestroyPrivateObjectSecurity, GENERIC_MAPPING,
    GetSecurityDescriptorLength, LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SEF_AVOID_OWNER_CHECK, SEF_AVOID_OWNER_RESTRICTION, SEF_AVOID_PRIVILEGE_CHECK,
    SEF_DACL_AUTO_INHERIT,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};

use super::object::{Object, os_error, reparse_refused, words};
use super::{NtfsAcl, READ_ALL, dacl_info, same_path};
use crate::windows::acl_bytes::{
    Acl, DACL_AUTO_INHERITED, DACL_PROTECTED, Descriptor, SACL_AUTO_INHERITED, SACL_PROTECTED,
    bare_child, broad_allow, build_descriptor, empty_acl, inherited_label, sid_bytes,
    without_inherited,
};
use crate::windows::acl_plan::Sid;
use crate::windows::writer::{ApplyFailure, Reach, Walked};

/// Files' generic rights, for working out inherited entries.
const MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    GenericRead: FILE_GENERIC_READ.0,
    GenericWrite: FILE_GENERIC_WRITE.0,
    GenericExecute: FILE_GENERIC_EXECUTE.0,
    GenericAll: FILE_ALL_ACCESS.0,
};

/// A folder the walk has checked and will list: its handle, its final path, and its descriptor
/// as its children inherit from it.
struct Folder {
    object: Object,
    path: Vec<u16>,
    descriptor: Vec<u8>,
}

/// The walk below `root` with `acl`'s token; see
/// [`AclWriter::propagate`](super::AclWriter::propagate).
pub(super) fn walk(
    acl: &NtfsAcl,
    root: &Path,
    reach: Reach,
    strict: Option<&[Sid]>,
) -> Result<Walked, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let top = Object::open_folder(root).map_err(io)?;
    let mut seen = Walked::default();
    if top.name_surrogate() {
        return Err(ApplyFailure::Changed);
    }
    if !top.folder {
        return Ok(seen);
    }
    if !same_path(&top, root).map_err(io)? {
        return Err(ApplyFailure::Changed);
    }
    let watched: Vec<Vec<u8>> = strict.unwrap_or_default().iter().map(sid_bytes).collect();
    let watched = strict.map(|_| watched.as_slice());
    let mut stack = vec![Folder {
        path: top.normalized().map_err(io)?,
        descriptor: top.security(READ_ALL).map_err(io)?,
        object: top,
    }];
    while let Some(folder) = stack.pop() {
        race::point(race::Stage::Listing, &folder.path);
        if folder.object.normalized().map_err(io)? != folder.path {
            return Err(ApplyFailure::Changed);
        }
        let children = folder.object.children().map_err(io)?;
        race::point(race::Stage::Listed, &folder.path);
        for child in children {
            if reach == Reach::FilesOnly && child.folder {
                continue;
            }
            let object = match race::open(&folder.object, &child) {
                Ok(object) => object,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                // A strict walk can't leave a child bare that it couldn't open as itself.
                Err(e) if reparse_refused(&e) && strict.is_some() => {
                    return Err(ApplyFailure::Changed);
                }
                Err(e) if reparse_refused(&e) => {
                    seen.skipped += 1;
                    continue;
                }
                Err(e) => return Err(io(e)),
            };
            if object.name_surrogate() || (!object.folder && object.links > 1) {
                seen.skipped += 1;
                continue;
            }
            if reach == Reach::FilesOnly && object.folder {
                continue;
            }
            // Listed as a file, a folder now: it was swapped since the listing.
            if object.folder && !child.folder {
                return Err(ApplyFailure::Changed);
            }
            let mut path = folder.path.clone();
            path.push(u16::from(b'\\'));
            path.extend_from_slice(&child.name);
            if object.normalized().map_err(io)? != path {
                return Err(ApplyFailure::Changed);
            }
            let current = object.security(READ_ALL).map_err(io)?;
            let after = settle(acl, &object, &folder.descriptor, &current, watched)?;
            seen.visited += 1;
            if after.is_some() {
                seen.written += 1;
            }
            if object.folder && reach == Reach::Tree {
                stack.push(Folder {
                    object,
                    path,
                    descriptor: after.unwrap_or(current),
                });
            }
        }
    }
    tracing::debug!(
        visited = seen.visited,
        written = seen.written,
        "tier B walked a tree"
    );
    Ok(seen)
}

/// One child of the walk: refuses what beats an inherited deny when `watched` is given,
/// then writes its DACL and label as its parent's entries make them, if they changed.
/// Returns its descriptor as written, or `None` when nothing changed.
fn settle(
    acl: &NtfsAcl,
    child: &Object,
    parent: &[u8],
    current: &[u8],
    watched: Option<&[Vec<u8>]>,
) -> Result<Option<Vec<u8>>, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let now = Descriptor::parse(current).map_err(io)?;
    if watched.is_some() && now.dacl_protected() {
        return Err(ApplyFailure::ProtectedChild);
    }
    // A DACL not protected loses its inherited entries before it is checked or worked out again,
    // and one not auto-inherited the explicit copies of them too; see [`without_inherited`].
    let creator = if now.dacl.is_some() && !now.dacl_protected() {
        trimmed(acl, parent, current, child.folder).map_err(io)?
    } else {
        current.to_vec()
    };
    if let Some(watched) = watched {
        let dacl = Descriptor::parse(&creator).map_err(io)?.dacl;
        let dacl = dacl.map(Acl::parse).transpose().map_err(io)?;
        if dacl.is_some_and(|dacl| broad_allow(&dacl, watched)) {
            return Err(ApplyFailure::BroadAllowChild);
        }
    }
    let mut changed = false;
    let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
    let inherited = inherit(acl, parent, &creator, child.folder).map_err(io)?;
    let new = Descriptor::parse(&inherited).map_err(io)?;
    let mut want = new.control & bits;
    if want & DACL_PROTECTED == 0 {
        want |= DACL_AUTO_INHERITED;
    }
    if let Some(dacl) = new.dacl
        && (new.dacl != now.dacl || want != now.control & bits)
    {
        let info = dacl_info(new.control);
        child
            .set(info, &build_descriptor(want, Some(dacl), None))
            .map_err(io)?;
        changed = true;
    }
    let above = Descriptor::parse(parent).map_err(io)?;
    let parent_sacl = above.sacl.map(Acl::parse).transpose().map_err(io)?;
    let child_sacl = now.sacl.map(Acl::parse).transpose().map_err(io)?;
    let label = inherited_label(
        parent_sacl.as_ref(),
        child_sacl.as_ref(),
        now.control,
        child.folder,
    );
    if let Some(sacl) = label {
        if !child.write_owner {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        let bits = now.control & (SACL_PROTECTED | SACL_AUTO_INHERITED);
        child
            .set(
                LABEL_SECURITY_INFORMATION,
                &build_descriptor(bits, None, Some(&sacl)),
            )
            .map_err(io)?;
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    child.security(READ_ALL).map(Some).map_err(io)
}

/// `child` (a descriptor) without its inherited entries and, unless it is auto-inherited, the
/// explicit copies of them ([`without_inherited`]), measured against what `parent` alone gives a
/// new child of its kind and owner ([`bare_child`] worked out from `parent`).
fn trimmed(acl: &NtfsAcl, parent: &[u8], child: &[u8], folder: bool) -> io::Result<Vec<u8>> {
    let fresh = inherit(acl, parent, &bare_child(child)?, folder)?;
    let due = Descriptor::parse(&fresh)?
        .dacl
        .map_or_else(empty_acl, <[u8]>::to_vec);
    without_inherited(child, &due)
}

/// `child`'s descriptor with its inherited entries worked out again from `parent`'s
/// inheritable ones: explicit entries kept, a protected DACL left as it is
/// (`CreatePrivateObjectSecurityEx` with `SEF_DACL_AUTO_INHERIT`, as Windows' own
/// propagation does). Only the DACL and its control bits of the result are used.
fn inherit(acl: &NtfsAcl, parent: &[u8], child: &[u8], folder: bool) -> io::Result<Vec<u8>> {
    let mut parent = words(parent);
    let mut child = words(child);
    let mut new = PSECURITY_DESCRIPTOR::default();
    let mapping = MAPPING;
    let flags = SEF_DACL_AUTO_INHERIT
        | SEF_AVOID_PRIVILEGE_CHECK
        | SEF_AVOID_OWNER_CHECK
        | SEF_AVOID_OWNER_RESTRICTION;
    // SAFETY: both descriptors are valid self-relative descriptors in buffers that live for
    // the call; `new` is a live out-pointer, freed below with DestroyPrivateObjectSecurity;
    // the token is open and the mapping is a constant.
    unsafe {
        CreatePrivateObjectSecurityEx(
            Some(PSECURITY_DESCRIPTOR(parent.as_mut_ptr().cast())),
            Some(PSECURITY_DESCRIPTOR(child.as_mut_ptr().cast())),
            &raw mut new,
            None,
            folder,
            flags,
            Some(acl.token.0),
            &raw const mapping,
        )
    }
    .map_err(|e| os_error(&e))?;
    // SAFETY: `new` is the valid self-relative descriptor the call just made.
    let len = unsafe { GetSecurityDescriptorLength(new) };
    let len = usize::try_from(len).map_err(io::Error::other)?;
    // SAFETY: the descriptor is `len` bytes long and lives until it is destroyed below.
    let bytes = unsafe { std::slice::from_raw_parts(new.0.cast::<u8>(), len) }.to_vec();
    // SAFETY: made by CreatePrivateObjectSecurityEx and destroyed once, here.
    if let Err(error) = unsafe { DestroyPrivateObjectSecurity(&raw const new) } {
        tracing::debug!(code = error.code().0, "DestroyPrivateObjectSecurity failed");
    }
    Ok(bytes)
}

/// `wide`, a final path, as a path.
#[cfg(test)]
pub(super) fn path_of(wide: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(wide))
}

/// The points in the walk where a test may change the tree, as an agent racing the walk would.
pub(super) mod race {
    /// Where the walk is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Stage {
        /// A folder is about to be checked and listed.
        Listing,
        /// A folder was listed; its children are about to be opened.
        Listed,
    }

    /// A test's hook, called with the stage and the folder's final path.
    #[cfg(test)]
    pub(crate) type Hook = Box<dyn FnMut(Stage, &std::path::Path)>;

    #[cfg(test)]
    thread_local! {
        /// The test's hook.
        pub(crate) static HOOK: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
    }

    /// Calls the test's hook, if one is set; nothing outside the tests.
    #[cfg(test)]
    pub(super) fn point(stage: Stage, folder: &[u16]) {
        let path = super::path_of(folder);
        HOOK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(stage, &path);
            }
        });
    }

    #[cfg(not(test))]
    pub(super) fn point(_stage: Stage, _folder: &[u16]) {}

    #[cfg(test)]
    thread_local! {
        /// A child name whose open fails as one that met a reparse point.
        pub(crate) static REFUSE: std::cell::RefCell<Option<std::ffi::OsString>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Opens `child` in `folder`; in the tests, the name [`REFUSE`] holds fails as an open
    /// that met a reparse point would.
    pub(super) fn open(
        folder: &super::Object,
        child: &super::super::object::Child,
    ) -> std::io::Result<super::Object> {
        #[cfg(test)]
        {
            use std::os::windows::ffi::OsStringExt as _;
            let name = std::ffi::OsString::from_wide(&child.name);
            if REFUSE.with(|refuse| refuse.borrow().as_ref() == Some(&name)) {
                return Err(std::io::Error::from_raw_os_error(4395));
            }
        }
        folder.open_child(&child.name, child.folder)
    }
}
