//! One item held to the strict walk's rule on its own (R18): the top of a read of another
//! matter, which no walk visits as a child ([`check_item`]).

use std::io;
use std::path::{Path, PathBuf};

use super::trimmed;
use crate::windows::acl::object::{Handle, Object};
use crate::windows::acl::{READ_ALL, final_form, reads_on, same_path};
use crate::windows::acl_bytes::{Acl, Descriptor, broad_allow_but, read_flags, sid_bytes};
use crate::windows::writer::{ApplyFailure, Strict};

/// Each read of another matter `strict` excuses, its path as final paths are written, with the
/// flags its allow is written with; a shape no read takes excuses nothing.
pub(in crate::windows::acl) fn listed_reads(
    strict: &Strict,
) -> io::Result<Vec<(PathBuf, Vec<u8>, u8)>> {
    strict
        .excused
        .iter()
        .filter_map(|(path, sid, inherit)| Some((path, sid, read_flags(*inherit)?)))
        .map(|(path, sid, flags)| Ok((final_form(path)?, sid_bytes(sid), flags)))
        .collect()
}

/// `path` itself, opened without following a link, held to `strict` as a strict walk holds a
/// child against its folder: a protected DACL is [`ApplyFailure::ProtectedChild`], an explicit
/// allow that beats an inherited deny (but a listed read) [`ApplyFailure::BroadAllowChild`].
/// Nothing is written or repaired, whoever owns it. A link there, or a final path other than
/// `path`, is [`ApplyFailure::Changed`].
pub(in crate::windows::acl) fn check_item(
    token: &Handle,
    path: &Path,
    strict: &Strict,
) -> Result<(), ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let target = Object::open(path).map_err(io)?;
    if target.name_surrogate() || !same_path(&target, path).map_err(io)? {
        return Err(ApplyFailure::Changed);
    }
    let current = target.security(READ_ALL).map_err(io)?;
    let now = Descriptor::parse(&current).map_err(io)?;
    if now.dacl_protected() {
        return Err(ApplyFailure::ProtectedChild);
    }
    if now.dacl.is_none() {
        return Ok(());
    }
    // Its inherited entries, and their explicit copies, measured against its folder's.
    let folder = path.parent().ok_or(ApplyFailure::Changed)?;
    let parent = Object::open(folder)
        .map_err(io)?
        .security(READ_ALL)
        .map_err(io)?;
    let checked = trimmed(token, &parent, &current, target.folder).map_err(io)?;
    let checked_dacl = Descriptor::parse(&checked).map_err(io)?.dacl;
    let Some(dacl) = checked_dacl.map(Acl::parse).transpose().map_err(io)? else {
        return Ok(());
    };
    let watched: Vec<Vec<u8>> = strict.watched.iter().map(sid_bytes).collect();
    let reads = listed_reads(strict).map_err(io)?;
    let here = reads_on(&target, &reads).map_err(io)?;
    if broad_allow_but(&dacl, &watched, &here) {
        return Err(ApplyFailure::BroadAllowChild);
    }
    Ok(())
}
