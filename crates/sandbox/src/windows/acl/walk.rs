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
    GetSecurityDescriptorLength, LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SEF_AVOID_OWNER_CHECK, SEF_AVOID_OWNER_RESTRICTION,
    SEF_AVOID_PRIVILEGE_CHECK, SEF_DACL_AUTO_INHERIT, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};

use super::object::{Handle, Object, os_error, reparse_refused, words};
use super::{READ_ALL, dacl_info, same_path};
use crate::windows::acl_bytes::{
    Acl, DACL_AUTO_INHERITED, DACL_PROTECTED, Descriptor, SACL_AUTO_INHERITED, SACL_PROTECTED,
    Write, bare_child, broad_allow, build_descriptor, empty_acl, explicit_label, inherited_label,
    lacking, owner_bytes, owner_descriptor, rebuild, without_broad, without_inherited, writes,
};
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

/// How a walk treats each child, besides working out its inherited entries.
#[derive(Debug, Clone, Copy)]
pub(super) enum Mode<'a> {
    /// Nothing more.
    Plain,
    /// Held to [`Strict`](crate::windows::writer::Strict): `watched` and `pool` as bytes.
    Strict {
        watched: &'a [Vec<u8>],
        pool: &'a [Vec<u8>],
    },
    /// `forget_matter`'s: what one of `pool` owns goes back to `me`, cleaned.
    Reclaim { pool: &'a [Vec<u8>], me: &'a [u8] },
}

impl Mode<'_> {
    /// Whether `descriptor`'s owner is one of the pool users this mode repairs or hands back.
    fn owned(self, descriptor: &[u8]) -> bool {
        let pool = match self {
            Mode::Plain => return false,
            Mode::Strict { pool, .. } | Mode::Reclaim { pool, .. } => pool,
        };
        owner_bytes(descriptor).is_some_and(|owner| pool.iter().any(|p| p == owner))
    }
}

/// Why a walk failed, and the child it refused when it was one (its final path).
pub(super) type Refused = (ApplyFailure, Option<Vec<u16>>);

/// The walk below `root` with `token`; see
/// [`AclWriter::propagate`](super::AclWriter::propagate).
pub(super) fn walk(
    token: &Handle,
    root: &Path,
    reach: Reach,
    mode: Mode<'_>,
) -> Result<Walked, Refused> {
    let io = |e: io::Error| (ApplyFailure::Io(e.kind()), None);
    let top = Object::open_folder(root).map_err(io)?;
    let mut seen = Walked::default();
    if top.name_surrogate() {
        return Err((ApplyFailure::Changed, None));
    }
    if !top.folder {
        return Ok(seen);
    }
    if !same_path(&top, root).map_err(io)? {
        return Err((ApplyFailure::Changed, None));
    }
    let strict = matches!(mode, Mode::Strict { .. });
    // A child the lawyer can't open refuses a strict walk and forget's reclaim walk alike, as
    // `ChildShut` naming it, so the caller sets it aside (S15 `PR3b` round 4).
    let shut_refuses = strict || matches!(mode, Mode::Reclaim { .. });
    let mut stack = vec![Folder {
        path: top.normalized().map_err(io)?,
        descriptor: top.security(READ_ALL).map_err(io)?,
        object: top,
    }];
    while let Some(folder) = stack.pop() {
        race::point(race::Stage::Listing, &folder.path);
        if folder.object.normalized().map_err(io)? != folder.path {
            return Err((ApplyFailure::Changed, None));
        }
        let children = folder.object.children().map_err(io)?;
        race::point(race::Stage::Listed, &folder.path);
        for child in children {
            if reach == Reach::FilesOnly && child.folder {
                continue;
            }
            let mut path = folder.path.clone();
            path.push(u16::from(b'\\'));
            path.extend_from_slice(&child.name);
            let refuse = |failure: ApplyFailure| (failure, Some(path.clone()));
            let object = match race::open(&folder.object, &child) {
                Ok(object) => object,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                // A strict walk can't leave a child bare that it couldn't open as itself.
                Err(e) if reparse_refused(&e) && strict => {
                    return Err(refuse(ApplyFailure::Changed));
                }
                Err(e) if reparse_refused(&e) => {
                    seen.skipped += 1;
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied && shut_refuses => {
                    return Err(refuse(ApplyFailure::ChildShut));
                }
                Err(e) => return Err(io(e)),
            };
            if object.name_surrogate() {
                seen.skipped += 1;
                continue;
            }
            // A file with another name elsewhere: its DACL belongs to the file, so one a pool
            // user owns is repaired or handed back like any other (N2); the rest are left bare.
            if !object.folder && object.links > 1 {
                let owner = object.security(OWNER_SECURITY_INFORMATION.0).map_err(io)?;
                if !mode.owned(&owner) {
                    seen.skipped += 1;
                    continue;
                }
            }
            if reach == Reach::FilesOnly && object.folder {
                continue;
            }
            // Listed as a file, a folder now: it was swapped since the listing.
            if object.folder && !child.folder {
                return Err(refuse(ApplyFailure::Changed));
            }
            if object.normalized().map_err(io)? != path {
                return Err(refuse(ApplyFailure::Changed));
            }
            let current = object.security(READ_ALL).map_err(io)?;
            let settled =
                settle(token, &object, &folder.descriptor, &current, mode).map_err(refuse)?;
            seen.visited += 1;
            if settled.repaired {
                seen.repaired += 1;
            }
            if settled.written.is_some() {
                seen.written += 1;
            }
            if object.folder && reach == Reach::Tree {
                stack.push(Folder {
                    object,
                    path,
                    descriptor: settled.written.unwrap_or(current),
                });
            }
        }
    }
    tracing::debug!(
        visited = seen.visited,
        written = seen.written,
        repaired = seen.repaired,
        "tier B walked a tree"
    );
    Ok(seen)
}

/// [`AclWriter::hand_back`](crate::windows::writer::AclWriter::hand_back) with `token`:
/// `path` itself, when one of `pool` owns it, settled as a child of its folder in
/// [`Mode::Reclaim`], forget's hand-back, so it goes to `me`.
pub(super) fn hand_back(
    token: &Handle,
    path: &Path,
    pool: &[Vec<u8>],
    me: &[u8],
) -> Result<bool, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let Some(folder) = path.parent() else {
        return Ok(false);
    };
    let object = match Object::open(path) {
        Ok(object) => object,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io(e)),
    };
    if object.name_surrogate() || !same_path(&object, path).map_err(io)? {
        return Ok(false);
    }
    let mode = Mode::Reclaim { pool, me };
    let current = object.security(READ_ALL).map_err(io)?;
    if !mode.owned(&current) {
        return Ok(false);
    }
    let parent = Object::open_folder(folder)
        .and_then(|folder| folder.security(READ_ALL))
        .map_err(io)?;
    settle(token, &object, &parent, &current, mode)?;
    Ok(true)
}

/// What [`settle`] did to one child.
struct Settled {
    /// Its descriptor as written, or `None` when nothing changed.
    written: Option<Vec<u8>>,
    /// Whether a pool user's child was repaired or reclaimed.
    repaired: bool,
}

/// One child of the walk: held to `mode` (refused, repaired or reclaimed), then its DACL,
/// owner and label written as its parent's entries and the mode make them, if they changed, in
/// [`writes`]' order. A child a pool user owns whose handle lacks a right one of those writes
/// needs is refused as [`ApplyFailure::ChildShut`] before any write, so it is set aside (N3).
fn settle(
    token: &Handle,
    child: &Object,
    parent: &[u8],
    current: &[u8],
    mode: Mode<'_>,
) -> Result<Settled, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let now = Descriptor::parse(current).map_err(io)?;
    // A DACL not protected loses its inherited entries before it is checked or worked out again,
    // and one not auto-inherited the explicit copies of them too; see [`without_inherited`].
    let checked = if now.dacl.is_some() && !now.dacl_protected() {
        trimmed(token, parent, current, child.folder).map_err(io)?
    } else {
        current.to_vec()
    };
    let checked_dacl = Descriptor::parse(&checked).map_err(io)?.dacl;
    let dacl = checked_dacl.map(Acl::parse).transpose().map_err(io)?;
    let owned = mode.owned(current);
    // The descriptor the inherited entries are worked out from, and whether it was cleaned.
    let mut base = checked.clone();
    let mut repaired = false;
    // The hand-back's new owner.
    let mut new_owner = None;
    match mode {
        Mode::Plain => {}
        Mode::Strict { watched, .. } => {
            let broad = dacl.as_ref().is_some_and(|d| broad_allow(d, watched));
            if now.dacl_protected() || broad {
                if !owned {
                    return Err(if now.dacl_protected() {
                        ApplyFailure::ProtectedChild
                    } else {
                        ApplyFailure::BroadAllowChild
                    });
                }
                let cleaned = dacl.as_ref().map(|d| without_broad(d, watched));
                base = rebuild(&checked, None, cleaned.as_deref(), DACL_PROTECTED).map_err(io)?;
                repaired = true;
            }
        }
        Mode::Reclaim { pool, me } => {
            if owned {
                let cleaned = dacl.as_ref().map(|d| without_broad(d, pool));
                base =
                    rebuild(&checked, Some(me), cleaned.as_deref(), DACL_PROTECTED).map_err(io)?;
                repaired = true;
                new_owner = Some(me);
            }
        }
    }
    let drop_label = new_owner.is_some();
    let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
    // A repaired DACL, protected no more and maybe with a new owner, is trimmed against it. The
    // repair leaves it marked auto-inherited, so whether its explicit copies go is decided by the
    // DACL as it was before the repair: the mark is cleared for the trim when it lacked it.
    if repaired && Descriptor::parse(&base).map_err(io)?.dacl.is_some() {
        let before = rebuild(&base, None, None, !now.control & DACL_AUTO_INHERITED).map_err(io)?;
        base = trimmed(token, parent, &before, child.folder).map_err(io)?;
    }
    let base_now = Descriptor::parse(&base).map_err(io)?;
    let inherited = inherit(token, parent, &base, child.folder).map_err(io)?;
    let new = Descriptor::parse(&inherited).map_err(io)?;
    let mut want = new.control & bits;
    if want & DACL_PROTECTED == 0 {
        want |= DACL_AUTO_INHERITED;
    }
    let dacl_write = new
        .dacl
        .filter(|_| new.dacl != now.dacl || want != now.control & bits)
        .map(|new_dacl| {
            let mut info = dacl_info(new.control);
            if now.dacl_protected() && new.control & DACL_PROTECTED == 0 {
                info |= UNPROTECTED_DACL_SECURITY_INFORMATION;
            }
            (info, build_descriptor(want, Some(new_dacl), None))
        });
    let above = Descriptor::parse(parent).map_err(io)?;
    let parent_sacl = above.sacl.map(Acl::parse).transpose().map_err(io)?;
    let child_sacl = now.sacl.map(Acl::parse).transpose().map_err(io)?;
    let has_explicit = child_sacl.as_ref().and_then(explicit_label).is_some();
    let label = if drop_label && has_explicit {
        let control = base_now.control & !SACL_PROTECTED;
        Some(
            inherited_label(parent_sacl.as_ref(), None, control, child.folder)
                .unwrap_or_else(empty_acl),
        )
    } else {
        inherited_label(
            parent_sacl.as_ref(),
            child_sacl.as_ref(),
            now.control,
            child.folder,
        )
    };
    let label_write = label.map(|sacl| {
        let keep = if drop_label {
            SACL_AUTO_INHERITED
        } else {
            SACL_PROTECTED | SACL_AUTO_INHERITED
        };
        build_descriptor(now.control & keep, None, Some(&sacl))
    });
    let order = writes(
        dacl_write.is_some(),
        new_owner.is_some(),
        label_write.is_some(),
    );
    if lacking(&order, child.write_dac, child.write_owner).is_some() {
        return Err(if owned {
            ApplyFailure::ChildShut
        } else {
            ApplyFailure::Io(io::ErrorKind::PermissionDenied)
        });
    }
    for write in &order {
        match (write, &dacl_write, new_owner, &label_write) {
            (Write::Dacl, Some((info, descriptor)), _, _) => child.set(*info, descriptor),
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
    Ok(Settled { written, repaired })
}

/// `child` (a descriptor) without its inherited entries and, unless it is auto-inherited, the
/// explicit copies of them ([`without_inherited`]), measured against what `parent` alone gives a
/// new child of its kind and owner ([`bare_child`] worked out from `parent`).
fn trimmed(token: &Handle, parent: &[u8], child: &[u8], folder: bool) -> io::Result<Vec<u8>> {
    let fresh = inherit(token, parent, &bare_child(child)?, folder)?;
    let due = Descriptor::parse(&fresh)?
        .dacl
        .map_or_else(empty_acl, <[u8]>::to_vec);
    without_inherited(child, &due)
}

/// `child`'s descriptor with its inherited entries worked out again from `parent`'s
/// inheritable ones: explicit entries kept, a protected DACL left as it is
/// (`CreatePrivateObjectSecurityEx` with `SEF_DACL_AUTO_INHERIT`, as Windows' own
/// propagation does). Only the DACL and its control bits of the result are used.
fn inherit(token: &Handle, parent: &[u8], child: &[u8], folder: bool) -> io::Result<Vec<u8>> {
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
            Some(token.0),
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
