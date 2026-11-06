// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/acl.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md
//! Tier B's permission writes on NTFS (S15 PR3): [`NtfsAcl`], the [`AclWriter`] the launcher
//! uses, and (in `walk.rs`) the walk that carries inherited entries down a tree.
//!
//! Derived from Codex's `codex-rs/windows-sandbox-rs/src/acl.rs` at `rust-v0.162.1`
//! (Apache-2.0, `OpenAI`; see `PROVENANCE.md` and `LICENSES/Apache-2.0-codex.txt`): every target
//! is opened once and read and written through that handle (`fetch_dacl_handle`,
//! `add_deny_ace`), a deny for the whole tree is refused on a volume's root
//! (`ensure_handle_is_not_filesystem_root`), and an entry already there is not written again.
//! Changed for tier B (`PROVENANCE.md` lists it): no link followed, the final path checked,
//! entries edited as bytes, `SetKernelObjectSecurity` (no propagation) plus Clauseo's own walk
//! (D7's skips, no hydration), and the Low label. The walk (`walk.rs`) opens every child
//! relative to its folder's handle (`object.rs`), so no path is looked up twice.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 file and security calls that write tier B's entries"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION,
    OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{WRITE_DAC, WRITE_OWNER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::acl_bytes::{
    AceSpec, Acl, DACL_AUTO_INHERITED, DACL_PROTECTED, Descriptor, SACL_AUTO_INHERITED,
    SACL_PROTECTED, build_descriptor, empty_acl, explicit_label, label_spec, owner_descriptor,
    owner_sid, sid_bytes,
};
use super::acl_plan::{Grant, Inherit, Sid, in_folder};
use super::admit::PathKind;
use super::folder::verbatim_wide;
use super::ledger::RecordedFile;
use super::record::current_user_sid;
use super::writer::{AclWriter, Added, ApplyFailure, Label, Reach, Strict, Walked};
use object::{Handle, Object, os_error};
use walk::{Foreign, Mode};

pub(in crate::windows) mod object;
mod walk;

/// What every read here asks for: owner, group, DACL and label, all under `READ_CONTROL`.
const READ_ALL: u32 = OWNER_SECURITY_INFORMATION.0
    | GROUP_SECURITY_INFORMATION.0
    | DACL_SECURITY_INFORMATION.0
    | LABEL_SECURITY_INFORMATION.0;

/// Tier B's permission writes on NTFS. Holds this process's token, which working out an
/// inherited descriptor asks for.
#[derive(Debug)]
pub struct NtfsAcl {
    token: Handle,
    /// The child the last failed walk refused, when it was one.
    refused: Option<PathBuf>,
}

impl NtfsAcl {
    /// Opens this process's token for querying.
    pub fn new() -> io::Result<NtfsAcl> {
        let mut token = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a
        // live out-pointer and the handle it receives is closed by `Handle`.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
            .map_err(|e| os_error(&e))?;
        Ok(NtfsAcl {
            token: Handle(token),
            refused: None,
        })
    }

    /// Runs the walk in `mode`, keeping the child it refused for [`AclWriter::refused_at`].
    fn run(&mut self, root: &Path, reach: Reach, mode: Mode<'_>) -> Result<Walked, ApplyFailure> {
        self.refused = None;
        walk::walk(&self.token, root, reach, mode).map_err(|(failure, at)| {
            self.refused = at.map(|wide| PathBuf::from(OsString::from_wide(&wide)));
            failure
        })
    }
}

impl AclWriter for NtfsAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        write(grant, None)
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        match open_planned(grant.path())? {
            Some(target) => remove_on(&target, grant),
            None => Ok(()),
        }
    }

    /// A target that is a link now, or whose final path isn't the planned one, is an error, so
    /// reconcile keeps its entry pending rather than judge it by another object.
    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        match open_planned(grant.path())? {
            Some(target) => present_on(&target, grant),
            None => Ok(false),
        }
    }

    /// Opened for `WRITE_DAC` to remove an allow or deny, `WRITE_OWNER` to remove or put back
    /// a label: no more than the removal needs.
    fn remove_recorded(
        &mut self,
        file: &RecordedFile,
        grant: &Grant,
        prior: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        let rights = if grant.rights().is_label() {
            WRITE_OWNER.0
        } else {
            WRITE_DAC.0
        };
        let opened = Object::open_by_id(&file.path, file.id, rights);
        let Some(target) = opened.map_err(|e| ApplyFailure::of(&e))? else {
            return Ok(());
        };
        remove_on(&target, grant)?;
        match prior {
            Some(label) => restore_on(&target, grant, label),
            None => Ok(()),
        }
    }

    fn present_recorded(&mut self, file: &RecordedFile, grant: &Grant) -> io::Result<bool> {
        match Object::open_by_id(&file.path, file.id, 0)? {
            Some(target) => present_on(&target, grant),
            None => Ok(false),
        }
    }

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        let Some(strict) = strict else {
            return self.run(root, reach, Mode::Plain);
        };
        let watched: Vec<Vec<u8>> = strict.watched.iter().map(sid_bytes).collect();
        let pool: Vec<Vec<u8>> = strict.pool.iter().map(sid_bytes).collect();
        // In a matter's tree, the desktop user another pool user's child is handed to (S15
        // `PR4h`): read once a walk, only there.
        let assigned: Vec<Vec<u8>> = strict.assigned.iter().map(sid_bytes).collect();
        let me = if assigned.is_empty() {
            Vec::new()
        } else {
            sid_bytes(&current_user_sid().map_err(|e| ApplyFailure::Io(e.kind()))?)
        };
        let mode = Mode::Strict {
            watched: &watched,
            pool: &pool,
            foreign: (!assigned.is_empty()).then_some(Foreign {
                assigned: &assigned,
                me: &me,
            }),
        };
        self.run(root, reach, mode)
    }

    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool> {
        let (a, b) = (Object::open(a)?, Object::open(b)?);
        if a.name_surrogate() || b.name_surrogate() {
            return Ok(false);
        }
        Ok(a.file_id()? == b.file_id()?)
    }

    fn file_id(&mut self, path: &Path) -> io::Result<Option<super::writer::FileId>> {
        let object = match Object::open(path) {
            Ok(object) => object,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if object.name_surrogate() {
            return Err(io::Error::from(ApplyFailure::Changed));
        }
        let (volume, id) = object.file_id()?;
        Ok(Some(super::writer::FileId { volume, id }))
    }

    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure> {
        write(grant, Some(expected))
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        self.refused.take()
    }

    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        let me = current_user_sid().map_err(|e| ApplyFailure::Io(e.kind()))?;
        let me = sid_bytes(&me);
        let pool: Vec<Vec<u8>> = pool.iter().map(sid_bytes).collect();
        self.run(
            root,
            Reach::Tree,
            Mode::Reclaim {
                pool: &pool,
                me: &me,
            },
        )
    }

    fn hand_back_tree(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        let me = current_user_sid().map_err(|e| ApplyFailure::Io(e.kind()))?;
        let me = sid_bytes(&me);
        let pool: Vec<Vec<u8>> = pool.iter().map(sid_bytes).collect();
        let mode = Mode::HandBack {
            pool: &pool,
            me: &me,
        };
        self.run(root, Reach::Tree, mode)
    }

    fn hand_back(&mut self, path: &Path, pool: &[Sid]) -> Result<bool, ApplyFailure> {
        let me = current_user_sid().map_err(|e| ApplyFailure::Io(e.kind()))?;
        let me = sid_bytes(&me);
        let pool: Vec<Vec<u8>> = pool.iter().map(sid_bytes).collect();
        walk::hand_back(&self.token, path, &pool, &me)
    }

    fn label(&mut self, path: &Path) -> io::Result<Option<Label>> {
        let Some(target) = open_planned(path)? else {
            return Ok(None);
        };
        let current = target.security(LABEL_SECURITY_INFORMATION.0)?;
        let now = Descriptor::parse(&current)?;
        let Some(sacl) = now.sacl else {
            return Ok(None);
        };
        Ok(explicit_label(&Acl::parse(sacl)?))
    }

    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure> {
        match open_planned(grant.path())? {
            Some(target) => restore_on(&target, grant, label),
            None => Ok(()),
        }
    }
}

/// Removes `grant`'s explicit entry from `target`; one already gone is `Ok`.
fn remove_on(target: &Object, grant: &Grant) -> Result<(), ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let spec = AceSpec::for_grant(grant, target.folder).map_err(io)?;
    let current = target.security(READ_ALL).map_err(io)?;
    let now = Descriptor::parse(&current).map_err(io)?;
    if grant.rights().is_label() {
        let Some(sacl) = now.sacl else { return Ok(()) };
        let Some(kept) = Acl::parse(sacl).map_err(io)?.without(&spec) else {
            return Ok(());
        };
        let bits = now.control & (SACL_PROTECTED | SACL_AUTO_INHERITED);
        let written = build_descriptor(bits, None, Some(&kept));
        target
            .set(LABEL_SECURITY_INFORMATION, &written)
            .map_err(io)?;
    } else {
        let Some(dacl) = now.dacl else { return Ok(()) };
        let Some(kept) = Acl::parse(dacl).map_err(io)?.without(&spec) else {
            return Ok(());
        };
        let bits = root_bits(now.control);
        let written = build_descriptor(bits, Some(&kept), None);
        target.set(dacl_info(now.control), &written).map_err(io)?;
    }
    Ok(())
}

/// Whether `grant`'s explicit entry is on `target`.
fn present_on(target: &Object, grant: &Grant) -> io::Result<bool> {
    let spec = AceSpec::for_grant(grant, target.folder)?;
    let current = target.security(READ_ALL)?;
    let now = Descriptor::parse(&current)?;
    let acl = if grant.rights().is_label() {
        now.sacl
    } else {
        now.dacl
    };
    Ok(acl
        .map(Acl::parse)
        .transpose()?
        .is_some_and(|acl| acl.has(&spec)))
}

/// Puts `label` back as the explicit label on `target` for label grant `grant`, as
/// [`AclWriter::restore_label`] does.
fn restore_on(target: &Object, grant: &Grant, label: Label) -> Result<(), ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let ours = AceSpec::for_grant(grant, target.folder).map_err(io)?;
    let current = target.security(READ_ALL).map_err(io)?;
    let now = Descriptor::parse(&current).map_err(io)?;
    let empty = empty_acl();
    let sacl = Acl::parse(now.sacl.unwrap_or(&empty)).map_err(io)?;
    // Another explicit label than ours is there: someone set it since; it stays.
    if let Some(there) = explicit_label(&sacl)
        && !sacl.has(&ours)
        && there != label
    {
        return Ok(());
    }
    let theirs = label_spec(label);
    if sacl.has(&theirs) {
        return Ok(());
    }
    if !target.write_owner {
        return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
    }
    let bits = now.control & (SACL_PROTECTED | SACL_AUTO_INHERITED);
    let written = build_descriptor(bits, None, Some(&sacl.with(&theirs)));
    target.set(LABEL_SECURITY_INFORMATION, &written).map_err(io)
}

/// The descriptor of `path` (owner, group, DACL and label), for the tests.
#[cfg(test)]
pub(crate) fn descriptor(path: &Path) -> io::Result<Vec<u8>> {
    Object::open(path)?.security(READ_ALL)
}

/// [`AclWriter::add`], and with `expected` [`AclWriter::add_expecting`]: the target opened
/// once and checked; with `expected`, compared by the file ID on that same handle with the file
/// `expected` names (opened without following; a link there is `Changed`); then written
/// through that handle (N8).
fn write(grant: &Grant, expected: Option<&Path>) -> Result<Added, ApplyFailure> {
    let io = |e: io::Error| ApplyFailure::Io(e.kind());
    let target = Object::open(grant.path()).map_err(io)?;
    check_target(&target, grant)?;
    if let Some(expected) = expected {
        let expected = Object::open(expected).map_err(io)?;
        if expected.name_surrogate()
            || target.file_id().map_err(io)? != expected.file_id().map_err(io)?
        {
            return Err(ApplyFailure::Changed);
        }
    }
    let spec = AceSpec::for_grant(grant, target.folder).map_err(io)?;
    let current = target.security(READ_ALL).map_err(io)?;
    let now = Descriptor::parse(&current).map_err(io)?;
    if grant.rights().is_label() {
        let empty = empty_acl();
        let sacl = Acl::parse(now.sacl.unwrap_or(&empty)).map_err(io)?;
        if sacl.has(&spec) {
            return Ok(Added::AlreadyThere);
        }
        if !target.write_owner {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        let bits = now.control & (SACL_PROTECTED | SACL_AUTO_INHERITED);
        let written = build_descriptor(bits, None, Some(&sacl.with(&spec)));
        target
            .set(LABEL_SECURITY_INFORMATION, &written)
            .map_err(io)?;
    } else {
        let dacl = Acl::parse(now.dacl.ok_or(ApplyFailure::NoDacl)?).map_err(io)?;
        if dacl.has(&spec) {
            return Ok(Added::AlreadyThere);
        }
        let bits = root_bits(now.control);
        let written = build_descriptor(bits, Some(&dacl.with(&spec)), None);
        target.set(dacl_info(now.control), &written).map_err(io)?;
    }
    Ok(Added::Written)
}

/// `path` opened without following a link, `None` when it is missing; a link there now, or
/// a final path other than `path`, is [`ApplyFailure::Changed`].
fn open_planned(path: &Path) -> Result<Option<Object>, ApplyFailure> {
    let target = match Object::open(path) {
        Ok(target) => target,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ApplyFailure::Io(e.kind())),
    };
    let same = same_path(&target, path).map_err(|e| ApplyFailure::Io(e.kind()))?;
    if target.name_surrogate() || !same {
        return Err(ApplyFailure::Changed);
    }
    Ok(Some(target))
}

/// `path`'s whole tree handed to this process's user, as forget hands back a pool user's
/// items: the item itself when one of `pool` owns it ([`AclWriter::hand_back`]), then every
/// item below it one of `pool` owns, whoever owns the top (the owner pass,
/// [`AclWriter::hand_back_tree`]: through each folder's handle, never following a link). Each
/// one handed back is worked out again from its folder. For the set-aside's moves and
/// `restore_set_aside`, so what the agent made is the lawyer's wherever it goes, even inside a
/// lawyer's folder it renamed to `.git` (S15 `PR4a2`). `pool` is the machine record's
/// (`sandbox_pool_sids`): what anyone else owns, a colleague's file the agent hard-linked in or
/// one `Administrators` owns, keeps its owner and DACL. An empty `pool` hands nothing over.
pub(crate) fn own(path: &Path, pool: &[Sid]) -> io::Result<()> {
    if pool.is_empty() {
        return Ok(());
    }
    let mut acl = NtfsAcl::new()?;
    acl.hand_back(path, pool).map_err(io::Error::from)?;
    acl.hand_back_tree(path, pool)
        .map(drop)
        .map_err(io::Error::from)
}

/// The owner of `path`, read without following a link: for the set-aside.
pub(crate) fn owner(path: &Path) -> io::Result<Sid> {
    let object = Object::open(path)?;
    let bytes = object.security(OWNER_SECURITY_INFORMATION.0)?;
    owner_sid(&bytes).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no owner"))
}

/// Makes `owner` the owner of `path` alone (`OWNER_SECURITY_INFORMATION` only, through
/// `SetKernelObjectSecurity` on its handle, so nothing below it changes): the set-up's hand-over
/// of a pool user's `LocalLow` to `Administrators` (S15 `PR4h`). Opened without following a
/// link, and refused, untouched, when `path` is a reparse point or the handle got no
/// `WRITE_OWNER`.
pub(crate) fn set_owner(path: &Path, owner: &Sid) -> io::Result<()> {
    let object = Object::open(path)?;
    if object.name_surrogate() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a junction or a link", path.display()),
        ));
    }
    if !object.write_owner {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{}: its owner can't be changed by this process",
                path.display()
            ),
        ));
    }
    object.set(
        OWNER_SECURITY_INFORMATION,
        &owner_descriptor(&sid_bytes(owner)),
    )
}

/// Refuses a target that isn't what the gate checked: a junction or symbolic link now; a file
/// with another name elsewhere, for an allow or label (a deny there only narrows pool users);
/// a final path other than the planned one; a deny for the whole tree on a volume's root.
fn check_target(target: &Object, grant: &Grant) -> Result<(), ApplyFailure> {
    if target.name_surrogate() {
        // A protected name in the matter that is a link: the lawyer's to move (S15 `PR4a2`).
        return Err(if grant.kind() == PathKind::ReadOnlyInMatter {
            ApplyFailure::Link
        } else {
            ApplyFailure::Changed
        });
    }
    let shared_ok = grant.rights().is_deny() || grant.kind() == PathKind::CredentialLink;
    if !target.folder && target.links > 1 && !shared_ok {
        return Err(ApplyFailure::LinkedFile);
    }
    if !same_path(target, grant.path()).map_err(|e| ApplyFailure::Io(e.kind()))? {
        return Err(ApplyFailure::Changed);
    }
    if grant.rights().is_deny()
        && matches!(grant.inherit(), Inherit::Tree | Inherit::Below)
        && target
            .is_volume_root()
            .map_err(|e| ApplyFailure::Io(e.kind()))?
    {
        return Err(ApplyFailure::VolumeRoot);
    }
    Ok(())
}

/// Whether `target`'s final path (`GetFinalPathNameByHandleW`, normalized, with its drive
/// letter, always `\\?\`) is `planned`, compared part by part without ASCII case. `planned`
/// is put in the same `\\?\` form it was opened through, rather than the final path stripped
/// of it: `dunce` keeps `\\?\` on a path longer than `MAX_PATH`, a share's path or a name it
/// can't write plainly, which made every such target `Changed`.
fn same_path(target: &Object, planned: &Path) -> io::Result<bool> {
    let final_path = PathBuf::from(OsString::from_wide(&target.normalized()?));
    let mut wide = verbatim_wide(planned)?;
    wide.pop();
    let planned = PathBuf::from(OsString::from_wide(&wide));
    Ok(in_folder(&final_path, &planned) && in_folder(&planned, &final_path))
}

/// The DACL control bits a write on a target keeps: its protection, and the auto-inherited mark
/// (`SE_DACL_AUTO_INHERITED`) set whether it was there or not. Its inherited entries stay as
/// they are. A target not marked so (a matter under a plain `C:\m`, the Windows run 2, row 15)
/// would otherwise keep a DACL whose inherited entries no later propagation can tell apart.
fn root_bits(control: u16) -> u16 {
    (control & DACL_PROTECTED) | DACL_AUTO_INHERITED
}

/// `DACL_SECURITY_INFORMATION`, with the protection kept when `control` has it.
fn dacl_info(control: u16) -> OBJECT_SECURITY_INFORMATION {
    if control & DACL_PROTECTED != 0 {
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        DACL_SECURITY_INFORMATION
    }
}

#[cfg(test)]
mod tests;
