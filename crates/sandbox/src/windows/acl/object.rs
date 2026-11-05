// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/acl.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1), and its no_reparse_dir.rs
// at e7f32cf1928a35a9232a633a51c2a03b5a7e2c72 (OBJ_DONT_REPARSE); see PROVENANCE.md
//! A file or folder opened for the permission writes, never through a link (S15 PR3): a target
//! or a walk's root by its path (`CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`), and a walk's
//! child by its name relative to its folder's handle (`NtCreateFile` with `RootDirectory` and
//! `FILE_OPEN_REPARSE_POINT`), so a name once checked is never looked up again through a path.
//! A folder is listed through its handle too (`FileIdBothDirectoryInfo`).

use std::io;
use std::path::Path;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    NtCreateFile,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, HANDLE, NTSTATUS,
    OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Security::{
    GetKernelObjectSecurity, OBJECT_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SetKernelObjectSecurity,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_ID_BOTH_DIR_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo, FileIdBothDirectoryInfo,
    FileIdBothDirectoryRestartInfo, FileIdInfo, FileStandardInfo, GETFINALPATHNAMEBYHANDLE_FLAGS,
    GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING, READ_CONTROL,
    SYNCHRONIZE, VOLUME_NAME_DOS, VOLUME_NAME_NONE, WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{PCWSTR, PWSTR};

use crate::windows::folder::verbatim_wide;

/// `IsReparseTagNameSurrogate`: the tag's name-surrogate bit (junctions, symbolic links).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// What a folder's listing needs besides the base rights.
const LIST: u32 = FILE_LIST_DIRECTORY.0 | SYNCHRONIZE.0;

/// A file or folder opened by its own name, never what a link names, with the rights it got.
#[derive(Debug)]
pub(in crate::windows) struct Object {
    handle: Handle,
    pub(in crate::windows) folder: bool,
    pub(in crate::windows) links: u32,
    tag: Option<u32>,
    pub(in crate::windows) write_owner: bool,
}

/// One name a folder's listing gave, and whether the listing said it is a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::windows) struct Child {
    pub(in crate::windows) name: Vec<u16>,
    pub(in crate::windows) folder: bool,
}

impl Object {
    /// Opens `path` itself (`FILE_FLAG_OPEN_REPARSE_POINT`; a cloud placeholder stays
    /// unhydrated) with `WRITE_DAC | WRITE_OWNER`, then without `WRITE_OWNER`, then read only.
    pub(in crate::windows) fn open(path: &Path) -> io::Result<Object> {
        let wide = verbatim_wide(path)?;
        ladder(0, |access| open_raw(&wide, access))
    }

    /// [`Object::open`], with the right to list it: a walk's root.
    pub(in crate::windows) fn open_folder(path: &Path) -> io::Result<Object> {
        let wide = verbatim_wide(path)?;
        ladder(LIST, |access| open_raw(&wide, access))
    }

    /// Opens `name` in this folder relative to its handle, never following a link, with the
    /// same rights as [`Object::open`]; and the right to list it when `folder` (the listing's
    /// word for it).
    pub(in crate::windows) fn open_child(&self, name: &[u16], folder: bool) -> io::Result<Object> {
        let extra = SYNCHRONIZE.0 | if folder { LIST } else { 0 };
        ladder(extra, |access| open_at(&self.handle, name, access))
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
    pub(in crate::windows) fn name_surrogate(&self) -> bool {
        self.tag.is_some_and(|tag| tag & NAME_SURROGATE != 0)
    }

    /// The names in this folder, through its handle, without `.` and `..`.
    pub(in crate::windows) fn children(&self) -> io::Result<Vec<Child>> {
        let mut buffer = vec![0_u64; 8192];
        let size = u32::try_from(buffer.len() * 8).map_err(io::Error::other)?;
        let mut class = FileIdBothDirectoryRestartInfo;
        let mut out = Vec::new();
        loop {
            // SAFETY: the handle is open and `buffer` is writable for `size` bytes.
            let read = unsafe {
                GetFileInformationByHandleEx(self.handle.0, class, buffer.as_mut_ptr().cast(), size)
            };
            class = FileIdBothDirectoryInfo;
            match read {
                Ok(()) => {}
                Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => return Ok(out),
                Err(e) => return Err(os_error(&e)),
            }
            // SAFETY: `buffer` is `size` initialised bytes, and u8 has no alignment.
            let bytes =
                unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size as usize) };
            listed(bytes, &mut out)?;
        }
    }

    /// Its security descriptor, self-relative, holding what `info` asks for.
    pub(in crate::windows) fn security(&self, info: u32) -> io::Result<Vec<u8>> {
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
    pub(in crate::windows) fn set(
        &self,
        info: OBJECT_SECURITY_INFORMATION,
        descriptor: &[u8],
    ) -> io::Result<()> {
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

    /// Its final path, normalized, with its drive letter (`\\?\C:\...`).
    pub(in crate::windows) fn normalized(&self) -> io::Result<Vec<u16>> {
        self.final_path(GETFINALPATHNAMEBYHANDLE_FLAGS(
            FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0,
        ))
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

    /// Its volume's serial number and its 128-bit file ID (`FileIdInfo`): the same file under
    /// any of its names.
    pub(in crate::windows) fn file_id(&self) -> io::Result<(u64, [u8; 16])> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the handle is open and `info` is valid and writable for its size.
        unsafe {
            GetFileInformationByHandleEx(
                self.handle.0,
                FileIdInfo,
                (&raw mut info).cast(),
                u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(0),
            )
        }
        .map_err(|e| os_error(&e))?;
        Ok((info.VolumeSerialNumber, info.FileId.Identifier))
    }

    /// Whether it is a volume's root: its path without the volume is `\` (Codex's
    /// `ensure_handle_is_not_filesystem_root`).
    pub(in crate::windows) fn is_volume_root(&self) -> io::Result<bool> {
        let path = self.final_path(GETFINALPATHNAMEBYHANDLE_FLAGS(VOLUME_NAME_NONE.0))?;
        Ok(path == [u16::from(b'\\')])
    }
}

/// Opens with `extra` and the base rights plus `WRITE_DAC | WRITE_OWNER`, then without
/// `WRITE_OWNER`, then read only, through `open`.
fn ladder(extra: u32, mut open: impl FnMut(u32) -> io::Result<Handle>) -> io::Result<Object> {
    let base = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0 | extra;
    let mut denied = io::Error::from(io::ErrorKind::PermissionDenied);
    for (access, write_owner) in [
        (base | WRITE_DAC.0 | WRITE_OWNER.0, true),
        (base | WRITE_DAC.0, false),
        (base, false),
    ] {
        match open(access) {
            Ok(handle) => return Object::describe(handle, write_owner),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => denied = e,
            Err(e) => return Err(e),
        }
    }
    Err(denied)
}

/// Adds the names in one `FILE_ID_BOTH_DIR_INFO` chain, `bytes`, to `out`.
fn listed(bytes: &[u8], out: &mut Vec<Child>) -> io::Result<()> {
    let attributes_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes);
    let length_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
    let name_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let short = || io::Error::new(io::ErrorKind::InvalidData, "a short folder listing");
    let mut at = 0_usize;
    loop {
        let entry = bytes.get(at..).ok_or_else(short)?;
        let next = read_u32(entry, 0).ok_or_else(short)?;
        let attributes = read_u32(entry, attributes_at).ok_or_else(short)?;
        let length = usize::try_from(read_u32(entry, length_at).ok_or_else(short)?)
            .map_err(io::Error::other)?;
        let raw = entry.get(name_at..name_at + length).ok_or_else(short)?;
        let name: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let dots = name == [u16::from(b'.')] || name == [u16::from(b'.'), u16::from(b'.')];
        if !dots && !name.is_empty() {
            out.push(Child {
                name,
                folder: attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
            });
        }
        if next == 0 {
            return Ok(());
        }
        at += usize::try_from(next).map_err(io::Error::other)?;
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
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

/// Opens `name` (one path part, no NUL) in the folder `dir` for `access`, relative to its
/// handle, opening a reparse point itself rather than what it names. `OBJ_DONT_REPARSE` as
/// well, after Codex's `no_reparse_dir.rs` (`open_no_reparse`, at e7f32cf): any reparse met
/// on the way fails the open ([`reparse_refused`]) rather than being followed.
fn open_at(dir: &Handle, name: &[u16], access: u32) -> io::Result<Handle> {
    let bytes = u16::try_from(name.len() * 2)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: dir.0,
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let options =
        FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_FOR_BACKUP_INTENT;
    // SAFETY: `unicode` points into `name`, which outlives the call and is never written;
    // `attributes` and the out-pointers are live; the folder's handle is open; the handle
    // made is owned by `Handle` from here.
    let done = unsafe {
        NtCreateFile(
            &raw mut handle,
            FILE_ACCESS_RIGHTS(access),
            &raw const attributes,
            &raw mut status,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            share,
            FILE_OPEN,
            options,
            None,
            0,
        )
    };
    if done.is_err() {
        return Err(nt_error(done));
    }
    Ok(Handle(handle))
}

/// `ERROR_REPARSE_POINT_ENCOUNTERED`: what `STATUS_REPARSE_POINT_ENCOUNTERED` maps to.
const REPARSE_ENCOUNTERED: i32 = 4395;

/// Whether an open failed because it met a reparse point it was told not to follow.
pub(in crate::windows) fn reparse_refused(error: &io::Error) -> bool {
    error.raw_os_error() == Some(REPARSE_ENCOUNTERED)
}

/// `bytes` in u64 words, so the descriptor's structures are aligned for the Win32 calls.
pub(in crate::windows) fn words(bytes: &[u8]) -> Vec<u64> {
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
pub(in crate::windows) struct Handle(pub(in crate::windows) HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW, NtCreateFile or OpenProcessToken and is
        // closed once, here.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// The Win32 error `error` carries, as an `io::Error` whose kind follows its code.
pub(in crate::windows) fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

/// The NT status `status`, as the Win32 error it maps to.
fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}
