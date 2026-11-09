//! The guard on what the pool may write: nothing written there runs, and the writer can't take
//! that off. macOS has the same rule: only system and runtime paths may be mapped executable.
//!
//! Each place comes from the plan's own allows, not from a list: every allow to the pool user
//! that lets it add files to a folder (`FILE_ADD_FILE`, through `Modify` down the tree or
//! [`Rights::CreateFiles`]), and every file allow that lets it write the file and run it. Today
//! that is the matter, state and notes folders, the session's temp folder and the Claude store
//! folder, and each of the Claude store's allowed files. The credential link is left out: its
//! allow ([`Rights::ReadWriteData`]) has no execute right, and the file is the lawyer's own.
//!
//! - On a folder, all three for files only ([`Inherit::FilesBelow`], object inherit, inherit
//!   only): [`Rights::DenyExecute`], `(D;OIIO;0x20;;;<sid>)`, so a folder's traverse (the same
//!   bit) stays; [`Rights::DenyPermissionChange`], `(D;OIIO;0xc0000;;;<sid>)`, each for every
//!   subject the allows name (the pool user, and the matter SID when set); then one
//!   [`Rights::OwnerModify`], `(A;OIIO;0x1301bf;;;OW)`. An owner's implied `WRITE_DAC` is
//!   granted before any deny is read, so without it a pool user owning a file it made could drop
//!   the inherited deny; with it, the owner holds only `Modify`, and the permission deny wins
//!   over a `CREATOR OWNER` allow inherited from `C:\` (which comes after the matter's own
//!   entries, from further up). Folders keep their owner's `WRITE_DAC`: Python's `os.mkdir(p,
//!   0o700)` (3.12.4 and later, so `tempfile.mkdtemp` and pip) passes its own protected DACL at
//!   creation, which NTFS refuses to a creator that wouldn't hold `WRITE_DAC` there. Such a
//!   folder takes no inherited deny; the strict walk's repair (`acl_bytes::without_guarded`)
//!   strips it, and `NoLowMandatoryLabelImages` on the agent refuses image loads from it (0049).
//! - On a file: [`Rights::DenyExecute`] on the file alone, explicit, so it comes before the
//!   file's explicit `Modify` allow.
//!
//! A deny blocks image loads as well as starting a program: an `.exe`, `.dll`, `.node` or `.pyd`
//! in one of these places doesn't load. What is already there gets the entries by the walk that
//! carries every inherited entry down (`apply::walk`). Removal takes them off with the plan's
//! other entries, through the ledger.

use std::path::PathBuf;

use super::{Grant, Inherit, OWNER_RIGHTS_SID, Plan, Rights, Sid};
use crate::Error;
use crate::windows::admit::PathKind;

/// `FILE_WRITE_DATA` on a file, `FILE_ADD_FILE` on a folder.
const FILE_ADD_FILE: u32 = 0x2;

/// `FILE_EXECUTE` on a file.
const FILE_EXECUTE: u32 = 0x20;

/// The guard on each place `plan`'s allows so far let the pool user write, as the module says.
/// Refuses only if [`OWNER_RIGHTS_SID`] doesn't parse.
pub(super) fn guard_writes(plan: &mut Plan<'_>) -> Result<(), Error> {
    let owner = Sid::parse(OWNER_RIGHTS_SID).map_err(|e| Error::Policy(e.to_string()))?;
    let pool = &plan.subjects.pool_user;
    let places: Vec<Place> = plan
        .grants
        .iter()
        .filter(|g| g.sid == *pool && g.rights.is_allow())
        .filter_map(place)
        .collect();
    let mut subjects = vec![pool.clone()];
    subjects.extend(plan.subjects.matter.iter().cloned());
    for Place { path, kind, folder } in places {
        if !folder {
            for sid in &subjects {
                let deny = Rights::DenyExecute;
                plan.push(&path, sid.clone(), deny, Inherit::ThisOnly, kind);
            }
            continue;
        }
        for sid in &subjects {
            let deny = Rights::DenyExecute;
            plan.push(&path, sid.clone(), deny, Inherit::FilesBelow, kind);
        }
        for sid in &subjects {
            let deny = Rights::DenyPermissionChange;
            plan.push(&path, sid.clone(), deny, Inherit::FilesBelow, kind);
        }
        let owner = owner.clone();
        plan.push(&path, owner, Rights::OwnerModify, Inherit::FilesBelow, kind);
    }
    Ok(())
}

/// One place the pool may write.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Place {
    path: PathBuf,
    kind: PathKind,
    /// A folder it may add files to, or else a file it may write and run.
    pub(super) folder: bool,
}

/// The place `allow` lets the pool write, if it does: a folder it may add files to (with
/// `FILE_ADD_FILE`), or a file whose allow gives both writing and running. An allow down the
/// tree is on a folder; one on the path alone is on a folder or a file as its kind says.
pub(super) fn place(allow: &Grant) -> Option<Place> {
    let mask = allow.rights.mask();
    let folder = allow.inherit != Inherit::ThisOnly || folder_kind(allow.kind);
    let guarded = mask & FILE_ADD_FILE != 0 && (folder || mask & FILE_EXECUTE != 0);
    guarded.then(|| Place {
        path: allow.path.clone(),
        kind: allow.kind,
        folder,
    })
}

/// Whether a path of `kind` is a folder: every kind but the credential link and the Claude
/// store's files, and the read-only names, which may be either and get only denies.
fn folder_kind(kind: PathKind) -> bool {
    match kind {
        PathKind::Matter
        | PathKind::State
        | PathKind::Notes
        | PathKind::TempFolder
        | PathKind::ClaudeStore
        | PathKind::Runtime
        | PathKind::Read
        | PathKind::AllowedFolder
        | PathKind::OtherMatter
        | PathKind::Sibling
        | PathKind::ParentFiles
        | PathKind::SharedWrite => true,
        PathKind::CredentialLink
        | PathKind::ClaudeStoreFile
        | PathKind::ReadOnlyInMatter
        | PathKind::ReadOnlyInState => false,
    }
}
