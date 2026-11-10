//! The guard's repair of a child the pool owns (`Policy::native_code_deny`): the explicit allows
//! that would let it run what it holds, or change permissions, despite the guard's inherited
//! denies. Python's `tempfile.mkdtemp` (3.12.4 and later) makes each folder with the protected
//! DACL `D:P(A;OICI;FA;;;OW)`: the walk lifts the protection, and this takes the allow off.

use std::io;

use super::{ALLOWED, ALLOWED_CALLBACK, Acl, DENIED, INHERIT_ONLY, INHERITED, OBJECT_INHERIT};
use super::{flags, kind, mask, render, sid};

/// `FILE_EXECUTE`: on a file, running it or mapping it as an image.
const FILE_EXECUTE: u32 = 0x20;
/// `WRITE_DAC | WRITE_OWNER`.
const PERMISSION_CHANGE: u32 = 0x000c_0000;
/// `GENERIC_ALL`, which an inherit-only entry may hold, mapped when it is inherited.
const GENERIC_ALL: u32 = 0x1000_0000;
/// `GENERIC_EXECUTE`, likewise.
const GENERIC_EXECUTE: u32 = 0x2000_0000;

/// Whether `dacl`, a child's (a folder when `folder`), holds an explicit allow for one of
/// `guarded` that [`without_guarded`] takes off.
pub(crate) fn guard_offends(dacl: &Acl<'_>, folder: bool, guarded: &[Vec<u8>]) -> bool {
    let denied = denies_running(dacl, guarded);
    dacl.aces()
        .iter()
        .any(|ace| offends(ace, folder, guarded, denied))
}

/// The ACL `dacl` (its bytes) without the explicit allows for one of `guarded` that let the
/// child's owner change permissions (`WRITE_DAC`, `WRITE_OWNER`, `GENERIC_ALL`), or let it run
/// what the child holds: on a folder, an execute right its files inherit (object inherit); on
/// a file, an execute right, unless the file also has an explicit deny of execute for one of
/// `guarded` (as each of the Claude store's allowed files has, ahead of the plan's own
/// `Modify`). Every other entry is kept, in order.
pub(crate) fn without_guarded(
    dacl: &[u8],
    folder: bool,
    guarded: &[Vec<u8>],
) -> io::Result<Vec<u8>> {
    let acl = Acl::parse(dacl)?;
    let denied = denies_running(&acl, guarded);
    let kept: Vec<&[u8]> = acl
        .aces()
        .iter()
        .copied()
        .filter(|ace| !offends(ace, folder, guarded, denied))
        .collect();
    Ok(render(acl.revision.max(2), &kept))
}

/// Whether `ace` is an explicit allow [`without_guarded`] takes off.
fn offends(ace: &[u8], folder: bool, guarded: &[Vec<u8>], denied: bool) -> bool {
    let named = sid(ace).is_some_and(|s| guarded.iter().any(|g| g == s));
    let Some(m) = mask(ace) else {
        return false;
    };
    let changes = m & (PERMISSION_CHANGE | GENERIC_ALL) != 0;
    let runs = m & (FILE_EXECUTE | GENERIC_EXECUTE) != 0;
    let reaches = if folder {
        flags(ace) & OBJECT_INHERIT != 0
    } else {
        !denied
    };
    flags(ace) & INHERITED == 0
        && matches!(kind(ace), ALLOWED | ALLOWED_CALLBACK)
        && named
        && (changes || (runs && reaches))
}

/// Whether `acl` holds an explicit deny of execute, on the object itself, for one of `guarded`.
fn denies_running(acl: &Acl<'_>, guarded: &[Vec<u8>]) -> bool {
    acl.aces().iter().any(|ace| {
        kind(ace) == DENIED
            && flags(ace) & (INHERITED | INHERIT_ONLY) == 0
            && mask(ace).is_some_and(|m| m & FILE_EXECUTE != 0)
            && sid(ace).is_some_and(|s| guarded.iter().any(|g| g == s))
    })
}
