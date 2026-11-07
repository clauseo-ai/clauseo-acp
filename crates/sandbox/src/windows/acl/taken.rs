//! [`AclWriter::strip_taken`](super::AclWriter::strip_taken) on NTFS (S15 `PR4i`): the lawyer's
//! hand-back of what `--reclaim` took. The lawyer owns each such item now, and an owner is
//! implied only `READ_CONTROL | WRITE_DAC`, so each item (the root too) is opened with exactly
//! those (`Object::open_owned`), relative to its folder's handle, never following a link: no
//! `SYNCHRONIZE`, no `FILE_READ_ATTRIBUTES`, which a DACL only the agent's user is in doesn't
//! give. An item the lawyer owns that still allows another pool user by an explicit entry gets,
//! in one write on that handle, its DACL without those entries; or, when that leaves it
//! protected with nothing in it, its folder's inherited entries alone, unprotected
//! (`acl_bytes::taken_write`): the inherited grants are worked out from the stripped item, so
//! it never holds a pool user's allow and the matter's grants at once. It is read back after.
//! Only then is a folder opened again to be listed (now that it inherits, the lawyer may), and
//! the walk goes on below it. A child the listing calls a reparse point is skipped, and so is a
//! file with more than one link. Nothing else is written.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Win32::Security::UNPROTECTED_DACL_SECURITY_INFORMATION;

use super::object::{Handle, Object, reparse_refused};
use super::walk::{Refused, inherit};
use super::{READ_ALL, dacl_info, same_path};
use crate::windows::acl_bytes::{Descriptor, TakenWrite, sid_bytes, stripped, taken_write};
use crate::windows::acl_plan::{Sid, in_folder};
use crate::windows::writer::{ApplyFailure, Owned, Walked};

/// The strip at `root` and, when `whole`, below it, as the writer's method says. `others` and
/// `me` as bytes.
pub(super) fn strip(
    token: &Handle,
    root: &Path,
    others: &[Vec<u8>],
    me: &[u8],
    keep: &[(PathBuf, Sid)],
    whole: bool,
) -> Result<Walked, Refused> {
    let io = |e: io::Error| (ApplyFailure::of(&e), None);
    let mut seen = Walked::default();
    let (Some(above), Some(name)) = (root.parent(), root.file_name()) else {
        return Ok(seen);
    };
    let parent = match Object::open(above) {
        Ok(parent) => parent,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(seen),
        Err(e) => return Err(io(e)),
    };
    if parent.name_surrogate() || !same_path(&parent, above).map_err(io)? {
        return Err((ApplyFailure::Changed, None));
    }
    let name: Vec<u16> = OsStr::encode_wide(name).collect();
    let mut path = parent.normalized().map_err(io)?;
    path.push(u16::from(b'\\'));
    path.extend_from_slice(&name);
    let at = |failure: ApplyFailure| (failure, Some(path.clone()));
    let top = match parent.open_owned(&name) {
        Ok(top) => top,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(seen),
        Err(e) if reparse_refused(&e) => {
            seen.skipped += 1;
            return Ok(seen);
        }
        Err(_denied) => return Err(at(ApplyFailure::ChildShut)),
    };
    let above = parent.security(READ_ALL).map_err(io)?;
    let item = Item {
        object: &top,
        path: &path,
        above: &above,
    };
    one(token, &item, others, me, keep, &mut seen).map_err(at)?;
    if !whole || !top.folder {
        return Ok(seen);
    }
    let Some(listed) = listable(&parent, &name, &path).map_err(at)? else {
        seen.skipped += 1;
        return Ok(seen);
    };
    let mut stack = vec![(listed, path)];
    while let Some((folder, path)) = stack.pop() {
        let shut = |at: &[u16]| (ApplyFailure::ChildShut, Some(at.to_vec()));
        if folder.normalized().map_err(io)? != path {
            return Err((ApplyFailure::Changed, Some(path)));
        }
        let above = folder.security(READ_ALL).map_err(io)?;
        for child in folder.children().map_err(|_denied| shut(&path))? {
            let mut at = path.clone();
            at.push(u16::from(b'\\'));
            at.extend_from_slice(&child.name);
            if child.reparse {
                seen.skipped += 1;
                continue;
            }
            let object = match folder.open_owned(&child.name) {
                Ok(object) => object,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) if reparse_refused(&e) => {
                    seen.skipped += 1;
                    continue;
                }
                Err(_denied) => return Err(shut(&at)),
            };
            let item = Item {
                object: &object,
                path: &at,
                above: &above,
            };
            one(token, &item, others, me, keep, &mut seen).map_err(|f| (f, Some(at.clone())))?;
            if object.folder {
                match listable(&folder, &child.name, &at) {
                    Ok(Some(listed)) => stack.push((listed, at)),
                    Ok(None) => seen.skipped += 1,
                    Err(failure) => return Err((failure, Some(at))),
                }
            }
        }
    }
    Ok(seen)
}

/// The folder `name` in `folder` opened again, with the right to list it, once its strip is
/// written: `None` when it turns out to be a junction or other name surrogate (not entered).
/// One the lawyer still can't list (it kept a protection that shuts the lawyer out) refuses as
/// [`ApplyFailure::ChildShut`]; one at another final path than `path` as `Changed`.
fn listable(folder: &Object, name: &[u16], path: &[u16]) -> Result<Option<Object>, ApplyFailure> {
    let listed = folder
        .open_child(name, true)
        .map_err(|_denied| ApplyFailure::ChildShut)?;
    if listed.name_surrogate() {
        return Ok(None);
    }
    if listed
        .normalized()
        .map_err(|e| ApplyFailure::Io(e.kind()))?
        != path
    {
        return Err(ApplyFailure::Changed);
    }
    Ok(Some(listed))
}

/// An item opened with its owner's rights alone, its final path, and its folder's descriptor
/// (what it inherits from).
struct Item<'a> {
    object: &'a Object,
    path: &'a [u16],
    above: &'a [u8],
}

/// One item: its one write when it needs one ([`taken_write`]), then read back.
fn one(
    token: &Handle,
    item: &Item<'_>,
    others: &[Vec<u8>],
    me: &[u8],
    keep: &[(PathBuf, Sid)],
    seen: &mut Walked,
) -> Result<(), ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let object = item.object;
    seen.visited += 1;
    if !object.folder && object.links > 1 {
        seen.skipped += 1;
        return Ok(());
    }
    // Our own allows on this path (the ledger's) stay whoever they name.
    let named = PathBuf::from(OsString::from_wide(item.path));
    let ours: Vec<Vec<u8>> = keep
        .iter()
        .filter(|(at, _)| in_folder(&named, at) && in_folder(at, &named))
        .map(|(_, sid)| sid_bytes(sid))
        .collect();
    let others: Vec<Vec<u8>> = others
        .iter()
        .filter(|o| !ours.contains(o))
        .cloned()
        .collect();
    let others = others.as_slice();
    let current = object.security(READ_ALL).map_err(io)?;
    let mut worked = |base: &[u8]| inherit(token, item.above, base, object.folder);
    let Some(write) = taken_write(&current, others, me, &mut worked).map_err(io)? else {
        return Ok(());
    };
    let (info, new, inheriting) = match write {
        TakenWrite::Stripped(new) => {
            let control = Descriptor::parse(&new).map_err(io)?.control;
            (dacl_info(control), new, false)
        }
        TakenWrite::Inheriting(new) => {
            let control = Descriptor::parse(&new).map_err(io)?.control;
            let info = dacl_info(control) | UNPROTECTED_DACL_SECURITY_INFORMATION;
            (info, new, true)
        }
    };
    object.set(info, &new).map_err(io)?;
    let after = object.security(READ_ALL).map_err(io)?;
    let left = stripped(&after, others, me).map_err(io)?.is_some();
    if left || (inheriting && Descriptor::parse(&after).map_err(io)?.dacl_protected()) {
        return Err(ApplyFailure::PoolOwned {
            owned: Owned::Child,
        });
    }
    seen.written += 1;
    seen.repaired += 1;
    Ok(())
}
