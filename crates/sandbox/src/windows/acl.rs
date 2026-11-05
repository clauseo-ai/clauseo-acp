// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/acl.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md
//! Tier B's permission writes on NTFS (S15 PR3): [`NtfsAcl`], the [`AclWriter`] the launcher
//! uses, and the walk that carries inherited entries down a tree.
//!
//! Derived from Codex's `codex-rs/windows-sandbox-rs/src/acl.rs` at `rust-v0.162.1`
//! (Apache-2.0, `OpenAI`; see `PROVENANCE.md` and `LICENSES/Apache-2.0-codex.txt`): every target
//! is opened once and read and written through that handle (`fetch_dacl_handle`,
//! `add_deny_ace`), a deny for the whole tree is refused on a volume's root
//! (`ensure_handle_is_not_filesystem_root`), and an entry already there is not written again.
//! Changed for tier B (`PROVENANCE.md` lists it): no link followed, the final path checked,
//! entries edited as bytes, `SetKernelObjectSecurity` (no propagation) plus Clauseo's own walk
//! (D7's skips, no hydration), and the Low label.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 file and security calls that write tier B's entries"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{
    CreatePrivateObjectSecurityEx, DACL_SECURITY_INFORMATION, DestroyPrivateObjectSecurity,
    GENERIC_MAPPING, GROUP_SECURITY_INFORMATION, GetKernelObjectSecurity,
    GetSecurityDescriptorLength, LABEL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SEF_AVOID_OWNER_CHECK, SEF_AVOID_OWNER_RESTRICTION, SEF_AVOID_PRIVILEGE_CHECK,
    SEF_DACL_AUTO_INHERIT, SetKernelObjectSecurity, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo,
    FileStandardInfo, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, OPEN_EXISTING, READ_CONTROL, VOLUME_NAME_DOS, VOLUME_NAME_NONE,
    WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::PCWSTR;

use super::acl_bytes::{
    AceSpec, Acl, DACL_AUTO_INHERITED, DACL_PROTECTED, Descriptor, SACL_AUTO_INHERITED,
    SACL_PROTECTED, broad_allow, build_descriptor, empty_acl, inherited_label, sid_bytes,
};
use super::acl_plan::{Grant, Inherit, Sid, in_folder};
use super::admit::PathKind;
use super::folder::verbatim_wide;
use super::writer::{AclWriter, Added, ApplyFailure, Reach, Walked};

/// `IsReparseTagNameSurrogate`: the tag's name-surrogate bit (junctions, symbolic links).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// What every read here asks for: owner, group, DACL and label, all under `READ_CONTROL`.
const READ_ALL: u32 = OWNER_SECURITY_INFORMATION.0
    | GROUP_SECURITY_INFORMATION.0
    | DACL_SECURITY_INFORMATION.0
    | LABEL_SECURITY_INFORMATION.0;

/// Files' generic rights, for working out inherited entries.
const MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    GenericRead: FILE_GENERIC_READ.0,
    GenericWrite: FILE_GENERIC_WRITE.0,
    GenericExecute: FILE_GENERIC_EXECUTE.0,
    GenericAll: FILE_ALL_ACCESS.0,
};

/// Tier B's permission writes on NTFS. Holds this process's token, which working out an
/// inherited descriptor asks for.
#[derive(Debug)]
pub struct NtfsAcl {
    token: Handle,
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
        })
    }

    /// `child`'s descriptor with its inherited entries worked out again from `parent`'s
    /// inheritable ones: explicit entries kept, a protected DACL left as it is
    /// (`CreatePrivateObjectSecurityEx` with `SEF_DACL_AUTO_INHERIT`, as Windows' own
    /// propagation does). Only the DACL and its control bits of the result are used.
    fn inherit(&self, parent: &[u8], child: &[u8], folder: bool) -> io::Result<Vec<u8>> {
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
                Some(self.token.0),
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

    /// The walk below `root`; see [`AclWriter::propagate`].
    fn walk(
        &self,
        root: &Path,
        reach: Reach,
        strict: Option<&[Sid]>,
    ) -> Result<Walked, ApplyFailure> {
        let io = |e: io::Error| ApplyFailure::Io(e.kind());
        let top = Object::open(root).map_err(io)?;
        let mut seen = Walked::default();
        if !top.folder || top.name_surrogate() {
            return Ok(seen);
        }
        let watched: Vec<Vec<u8>> = strict.unwrap_or_default().iter().map(sid_bytes).collect();
        let mut stack: Vec<(PathBuf, Vec<u8>)> =
            vec![(root.to_owned(), top.security(READ_ALL).map_err(io)?)];
        while let Some((folder, parent)) = stack.pop() {
            for entry in std::fs::read_dir(&folder).map_err(io)? {
                let path = entry.map_err(io)?.path();
                let child = match Object::open(&path) {
                    Ok(child) => child,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(io(e)),
                };
                if child.name_surrogate() || (!child.folder && child.links > 1) {
                    seen.skipped += 1;
                    continue;
                }
                if reach == Reach::FilesOnly && child.folder {
                    continue;
                }
                let current = child.security(READ_ALL).map_err(io)?;
                let after = self.settle(&child, &parent, &current, strict.map(|_| &watched[..]))?;
                seen.visited += 1;
                if after.is_some() {
                    seen.written += 1;
                }
                if child.folder && reach == Reach::Tree {
                    stack.push((path, after.unwrap_or(current)));
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
        &self,
        child: &Object,
        parent: &[u8],
        current: &[u8],
        watched: Option<&[Vec<u8>]>,
    ) -> Result<Option<Vec<u8>>, ApplyFailure> {
        let io = |e: io::Error| ApplyFailure::Io(e.kind());
        let now = Descriptor::parse(current).map_err(io)?;
        if let Some(watched) = watched {
            if now.dacl_protected() {
                return Err(ApplyFailure::ProtectedChild);
            }
            let dacl = now.dacl.map(Acl::parse).transpose().map_err(io)?;
            if dacl.is_some_and(|dacl| broad_allow(&dacl, watched)) {
                return Err(ApplyFailure::BroadAllowChild);
            }
        }
        let mut changed = false;
        let inherited = self.inherit(parent, current, child.folder).map_err(io)?;
        let new = Descriptor::parse(&inherited).map_err(io)?;
        let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
        if let Some(dacl) = new.dacl
            && (new.dacl != now.dacl || new.control & bits != now.control & bits)
        {
            let info = dacl_info(new.control);
            child
                .set(
                    info,
                    &build_descriptor(new.control & bits, Some(dacl), None),
                )
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
}

impl AclWriter for NtfsAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        let io = |e: io::Error| ApplyFailure::Io(e.kind());
        let target = Object::open(grant.path()).map_err(io)?;
        check_target(&target, grant)?;
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
            let bits = now.control & (DACL_PROTECTED | DACL_AUTO_INHERITED);
            let written = build_descriptor(bits, Some(&dacl.with(&spec)), None);
            target.set(dacl_info(now.control), &written).map_err(io)?;
        }
        Ok(Added::Written)
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        let io = |e: io::Error| ApplyFailure::Io(e.kind());
        let target = match Object::open(grant.path()) {
            Ok(target) => target,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io(e)),
        };
        if target.name_surrogate() || !same_path(&target, grant.path()).map_err(io)? {
            return Err(ApplyFailure::Changed);
        }
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
            let bits = now.control & (DACL_PROTECTED | DACL_AUTO_INHERITED);
            let written = build_descriptor(bits, Some(&kept), None);
            target.set(dacl_info(now.control), &written).map_err(io)?;
        }
        Ok(())
    }

    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        let target = match Object::open(grant.path()) {
            Ok(target) => target,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
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

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&[Sid]>,
    ) -> Result<Walked, ApplyFailure> {
        self.walk(root, reach, strict)
    }
}

/// The descriptor of `path` (owner, group, DACL and label), for the tests.
#[cfg(test)]
pub(crate) fn descriptor(path: &Path) -> io::Result<Vec<u8>> {
    Object::open(path)?.security(READ_ALL)
}

/// Refuses a target that isn't what the gate checked: a junction or symbolic link now; a file
/// with another name elsewhere, for an allow or label (a deny there only narrows pool users);
/// a final path other than the planned one; a deny for the whole tree on a volume's root.
fn check_target(target: &Object, grant: &Grant) -> Result<(), ApplyFailure> {
    if target.name_surrogate() {
        return Err(ApplyFailure::Changed);
    }
    let shared_ok = grant.rights().is_deny() || grant.kind() == PathKind::CredentialLink;
    if !target.folder && target.links > 1 && !shared_ok {
        return Err(ApplyFailure::LinkedFile);
    }
    if !same_path(target, grant.path()).map_err(|e| ApplyFailure::Io(e.kind()))? {
        return Err(ApplyFailure::Changed);
    }
    if grant.rights().is_deny()
        && grant.inherit() == Inherit::Tree
        && target
            .is_volume_root()
            .map_err(|e| ApplyFailure::Io(e.kind()))?
    {
        return Err(ApplyFailure::VolumeRoot);
    }
    Ok(())
}

/// Whether `target`'s final path (`GetFinalPathNameByHandleW`, normalized, with its drive
/// letter) is `planned`, compared part by part without ASCII case, `\\?\` set aside.
fn same_path(target: &Object, planned: &Path) -> io::Result<bool> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);
    let final_path = PathBuf::from(OsString::from_wide(&target.final_path(flags)?));
    let final_path = dunce::simplified(&final_path);
    let planned = std::path::absolute(planned)?;
    let planned = dunce::simplified(&planned);
    Ok(in_folder(final_path, planned) && in_folder(planned, final_path))
}

/// `DACL_SECURITY_INFORMATION`, with the protection kept when `control` has it.
fn dacl_info(control: u16) -> OBJECT_SECURITY_INFORMATION {
    if control & DACL_PROTECTED != 0 {
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        DACL_SECURITY_INFORMATION
    }
}

/// A file or folder opened by its own name, never what a link names, with the rights it got.
#[derive(Debug)]
struct Object {
    handle: Handle,
    folder: bool,
    links: u32,
    tag: Option<u32>,
    write_owner: bool,
}

impl Object {
    /// Opens `path` itself (`FILE_FLAG_OPEN_REPARSE_POINT`; a cloud placeholder stays
    /// unhydrated) with `WRITE_DAC | WRITE_OWNER`, then without `WRITE_OWNER`, then read only.
    fn open(path: &Path) -> io::Result<Object> {
        let wide = verbatim_wide(path)?;
        let base = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
        let mut denied = io::Error::from(io::ErrorKind::PermissionDenied);
        for (access, write_owner) in [
            (base | WRITE_DAC.0 | WRITE_OWNER.0, true),
            (base | WRITE_DAC.0, false),
            (base, false),
        ] {
            match open_raw(&wide, access) {
                Ok(handle) => return Object::describe(handle, write_owner),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => denied = e,
                Err(e) => return Err(e),
            }
        }
        Err(denied)
    }

    fn describe(handle: Handle, write_owner: bool) -> io::Result<Object> {
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        let mut standard = FILE_STANDARD_INFO::default();
        // SAFETY: the handle is open and each struct is valid and writable for its size.
        unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileAttributeTagInfo,
                (&raw mut tag).cast(),
                u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
            )
            .map_err(|e| os_error(&e))?;
            GetFileInformationByHandleEx(
                handle.0,
                FileStandardInfo,
                (&raw mut standard).cast(),
                u32::try_from(size_of::<FILE_STANDARD_INFO>()).unwrap_or(0),
            )
            .map_err(|e| os_error(&e))?;
        }
        let reparse = tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
        Ok(Object {
            handle,
            folder: standard.Directory,
            links: standard.NumberOfLinks,
            tag: reparse.then_some(tag.ReparseTag),
            write_owner,
        })
    }

    /// Whether it is a junction, symbolic link or other name surrogate.
    fn name_surrogate(&self) -> bool {
        self.tag.is_some_and(|tag| tag & NAME_SURROGATE != 0)
    }

    /// Its security descriptor, self-relative, holding what `info` asks for.
    fn security(&self, info: u32) -> io::Result<Vec<u8>> {
        let mut size = 256_u32;
        for _ in 0..4 {
            let mut buffer = vec![0_u64; usize::try_from(size).unwrap_or(0).div_ceil(8)];
            let mut needed = 0_u32;
            // SAFETY: the handle is open; `buffer` is writable for `size` bytes or more, and
            // `needed` is a live out-pointer.
            let read = unsafe {
                GetKernelObjectSecurity(
                    self.handle.0,
                    info,
                    Some(PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast())),
                    size,
                    &raw mut needed,
                )
            };
            match read {
                Ok(()) => {
                    let mut bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_ne_bytes()).collect();
                    bytes.truncate(usize::try_from(size).unwrap_or(0));
                    return Ok(bytes);
                }
                Err(e) if e.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => size = needed,
                Err(e) => return Err(os_error(&e)),
            }
        }
        Err(io::Error::other(
            "a security descriptor kept growing while it was read",
        ))
    }

    /// Writes the parts `info` names from the self-relative descriptor `descriptor`, on this
    /// object alone (`SetKernelObjectSecurity` doesn't propagate).
    fn set(&self, info: OBJECT_SECURITY_INFORMATION, descriptor: &[u8]) -> io::Result<()> {
        let mut buffer = words(descriptor);
        // SAFETY: the handle is open; `buffer` holds a valid self-relative descriptor and lives
        // for the call.
        unsafe {
            SetKernelObjectSecurity(
                self.handle.0,
                info,
                PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast()),
            )
        }
        .map_err(|e| os_error(&e))
    }

    /// Its final path, as `flags` asks.
    fn final_path(&self, flags: GETFINALPATHNAMEBYHANDLE_FLAGS) -> io::Result<Vec<u16>> {
        let mut buffer = vec![0u16; 512];
        loop {
            // SAFETY: the handle is open and `buffer` is a writable slice of its length.
            let length = unsafe { GetFinalPathNameByHandleW(self.handle.0, &mut buffer, flags) };
            match usize::try_from(length).map_err(io::Error::other)? {
                0 => return Err(io::Error::last_os_error()),
                n if n < buffer.len() => {
                    buffer.truncate(n);
                    return Ok(buffer);
                }
                n => buffer.resize(n + 1, 0),
            }
        }
    }

    /// Whether it is a volume's root: its path without the volume is `\` (Codex's
    /// `ensure_handle_is_not_filesystem_root`).
    fn is_volume_root(&self) -> io::Result<bool> {
        let path = self.final_path(GETFINALPATHNAMEBYHANDLE_FLAGS(VOLUME_NAME_NONE.0))?;
        Ok(path == [u16::from(b'\\')])
    }
}

/// Opens `wide` (verbatim, NUL-terminated) for `access`, never following a link.
fn open_raw(wide: &[u16], access: u32) -> io::Result<Handle> {
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(Handle)
    .map_err(|e| os_error(&e))
}

/// `bytes` in u64 words, so the descriptor's structures are aligned for the Win32 calls.
fn words(bytes: &[u8]) -> Vec<u64> {
    bytes
        .chunks(8)
        .map(|chunk| {
            let mut word = [0u8; 8];
            if let Some(slot) = word.get_mut(..chunk.len()) {
                slot.copy_from_slice(chunk);
            }
            u64::from_ne_bytes(word)
        })
        .collect()
}

/// A handle, closed on drop.
#[derive(Debug)]
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW or OpenProcessToken and is closed once, here.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// The Win32 error `error` carries, as an `io::Error` whose kind follows its code.
fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

#[cfg(test)]
mod tests;
