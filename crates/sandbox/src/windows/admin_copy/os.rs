//! [`AdminCopyFs`] on Windows, and the admin folder's lock-down `--adopt` runs. Every path is
//! opened as itself (`FILE_FLAG_OPEN_REPARSE_POINT`), never what a link at it points at.
#![expect(
    unsafe_code,
    reason = "FFI to SHGetKnownFolderPath, CreateFileW and the security descriptor calls"
)]

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _};
use std::iter::once;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use sha2::{Digest as _, Sha256};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, GetSecurityDescriptorDacl,
    GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, OPEN_EXISTING, READ_CONTROL, VOLUME_NAME_DOS, WRITE_DAC,
    WRITE_OWNER,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{BOOL, PCWSTR, PWSTR};

use super::{AdminCopyFs, Found, Held, LOCKED_FILE_SDDL, LOCKED_FOLDER_SDDL, locked};
use crate::windows::record::{
    admin_copy_owner, admin_copy_sha256, current_user_sid, sandbox_owner_sid,
};
use crate::windows::setup::record_values::MACHINE_OWNER;

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;

/// The machine's admin copy, as [`super::check`] reads it.
pub(crate) struct WindowsAdminCopy;

impl AdminCopyFs for WindowsAdminCopy {
    fn program_files(&self) -> io::Result<PathBuf> {
        program_files()
    }

    fn found(&self, path: &Path) -> io::Result<Found> {
        found(path)
    }

    fn security(&self, path: &Path) -> io::Result<Vec<u8>> {
        security(path)
    }

    fn sha256(&self, path: &Path) -> io::Result<String> {
        sha256(path)
    }

    fn sha256_held(&self, path: &Path) -> io::Result<(String, Held)> {
        sha256_held(path)
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        admin_copy_sha256().map_err(io::Error::other)
    }

    fn security_blocked(&self) -> io::Result<bool> {
        super::mark_os::read()
    }

    fn set_security_blocked(&self, blocked: bool) -> io::Result<()> {
        if blocked {
            super::mark_os::record()
        } else {
            super::mark_os::delete()
        }
    }

    fn another_owner(&self) -> io::Result<bool> {
        let me = current_user_sid()?;
        let pool = sandbox_owner_sid().map_err(io::Error::other)?;
        let copy = admin_copy_owner().map_err(io::Error::other)?;
        Ok(pool.is_some_and(|owner| owner != me.as_str())
            || copy.is_some_and(|owner| owner != MACHINE_OWNER && owner != me.as_str()))
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<String>> {
        std::fs::read_dir(dir)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect()
    }
}

/// `--adopt`'s lock-down of the admin folder `dir`, whose copy is `exe`
/// (`take_back::lock_down_with`): `staging\` deleted, every entry but the copy and its signature
/// deleted, a link deleted as itself and never followed, anything that shuts Administrators out
/// taken back first; then `dir` and everything left in it given the locked owner and DACL, read
/// back exactly ([`lock_down`]), so no older file keeps its own entries or owner.
pub(crate) fn lock_down_tree(dir: &Path, exe: &Path) -> io::Result<()> {
    super::take_back::lock_down_with(&mut super::take_back_os::NtfsLockDown, dir, exe)
}

/// [`lock_down`] on `dir` and, below it, on every folder and file but `skip` (a name in `dir`);
/// a link below is deleted.
#[expect(
    clippy::disallowed_methods,
    reason = "deletes a link in the admin folder, only administrators' to change"
)]
pub(crate) fn lock_tree(dir: &Path, skip: Option<&str>) -> io::Result<()> {
    lock_down(dir, true)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if skip.is_some_and(|skip| entry.file_name().eq_ignore_ascii_case(skip)) {
            continue;
        }
        let path = entry.path();
        match found(&path)? {
            Found::Folder => lock_tree(&path, None)?,
            Found::File => lock_down(&path, false)?,
            Found::Link => {
                std::fs::remove_file(&path).or_else(|_file| std::fs::remove_dir(&path))?;
            }
            Found::Missing => {}
        }
    }
    Ok(())
}

/// [`WindowsAdminCopy`] for `UserSetup::state`, which the app asks at every tier query: the
/// copy's hash is kept for its file identity (volume, file ID, size and last write, read from
/// the handle it is hashed through) and computed again only when that changes. Every other read
/// happens each time. The launch's and the elevated start's checks never use it.
pub(crate) struct StateAdminCopy;

impl AdminCopyFs for StateAdminCopy {
    fn program_files(&self) -> io::Result<PathBuf> {
        WindowsAdminCopy.program_files()
    }

    fn found(&self, path: &Path) -> io::Result<Found> {
        WindowsAdminCopy.found(path)
    }

    fn security(&self, path: &Path) -> io::Result<Vec<u8>> {
        WindowsAdminCopy.security(path)
    }

    fn sha256(&self, path: &Path) -> io::Result<String> {
        sha256_cached(path)
    }

    fn sha256_held(&self, path: &Path) -> io::Result<(String, Held)> {
        WindowsAdminCopy.sha256_held(path)
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        WindowsAdminCopy.recorded_sha256()
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<String>> {
        WindowsAdminCopy.entries(dir)
    }

    fn another_owner(&self) -> io::Result<bool> {
        WindowsAdminCopy.another_owner()
    }

    fn security_blocked(&self) -> io::Result<bool> {
        WindowsAdminCopy.security_blocked()
    }

    fn set_security_blocked(&self, blocked: bool) -> io::Result<()> {
        WindowsAdminCopy.set_security_blocked(blocked)
    }
}

/// A file's identity for [`sha256_cached`]: volume serial, file index, size, last write time.
type Identity = (u32, u64, u64, u64);

/// The last hash [`sha256_cached`] computed, with the identity of the file it hashed.
static HASHED: std::sync::Mutex<Option<(Identity, String)>> = std::sync::Mutex::new(None);

/// [`sha256`], kept for the file's [`Identity`] as read from the handle it is hashed through.
fn sha256_cached(path: &Path) -> io::Result<String> {
    let mut file = open_regular(path)?;
    let identity = identity(&file)?;
    let mut last = HASHED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((seen, hash)) = last.as_ref()
        && *seen == identity
    {
        return Ok(hash.clone());
    }
    let hash = hash_file(&mut file)?;
    *last = Some((identity, hash.clone()));
    Ok(hash)
}

/// The identity of the open `file` (`GetFileInformationByHandle`).
fn identity(file: &File) -> io::Result<Identity> {
    use std::os::windows::io::AsRawHandle as _;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is `file`'s, open for the call; `info` is a live, writable struct.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut info) }
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    let pair = |high: u32, low: u32| u64::from(high) << 32 | u64::from(low);
    Ok((
        info.dwVolumeSerialNumber,
        pair(info.nFileIndexHigh, info.nFileIndexLow),
        pair(info.nFileSizeHigh, info.nFileSizeLow),
        pair(
            info.ftLastWriteTime.dwHighDateTime,
            info.ftLastWriteTime.dwLowDateTime,
        ),
    ))
}

/// `%ProgramFiles%` as Windows knows it (`FOLDERID_ProgramFiles`), never from the environment,
/// which whoever started this process could have set.
pub(crate) fn program_files() -> io::Result<PathBuf> {
    let folder = FOLDERID_ProgramFiles;
    // SAFETY: `folder` is a live GUID for the call; no token is passed (Program Files is the
    // machine's). The string returned is freed by `TaskString`.
    let path = unsafe { SHGetKnownFolderPath(&raw const folder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    let path = TaskString(path);
    // SAFETY: on success the call wrote a NUL-terminated string, alive until `path` drops.
    let units = unsafe { path.0.as_wide() };
    if units.is_empty() {
        return Err(io::Error::other("Windows gave no Program Files folder"));
    }
    Ok(PathBuf::from(OsString::from_wide(units)))
}

/// What is at `path`, from its own attributes (`symlink_metadata` never follows a link).
pub(crate) fn found(path: &Path) -> io::Result<Found> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_attributes() & REPARSE_POINT != 0 => Ok(Found::Link),
        Ok(meta) if meta.is_dir() => Ok(Found::Folder),
        Ok(meta) if meta.is_file() => Ok(Found::File),
        Ok(_) => Ok(Found::Link),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Found::Missing),
        Err(error) => Err(error),
    }
}

/// The SHA-256 of the regular file at `path`, read through one handle opened on the name
/// itself; a reparse point is refused from that handle's own attributes.
pub(crate) fn sha256(path: &Path) -> io::Result<String> {
    hash_file(&mut open_regular(path)?)
}

/// [`sha256`] through a handle that shares read only (no `FILE_SHARE_WRITE`, no
/// `FILE_SHARE_DELETE`), kept open in the [`Held`]: until it drops, no one can open the file
/// to write it, rename it or delete it. Starting it is a read, which the share allows.
pub(crate) fn sha256_held(path: &Path) -> io::Result<(String, Held)> {
    let mut file = open_shared(path, FILE_SHARE_READ.0)?;
    let hash = hash_file(&mut file)?;
    Ok((hash, Held::new(file)))
}

/// `path` opened as itself to read (`FILE_FLAG_OPEN_REPARSE_POINT`), refused unless the handle
/// is a regular file and no reparse point.
fn open_regular(path: &Path) -> io::Result<File> {
    open_shared(path, (FILE_SHARE_READ | FILE_SHARE_DELETE).0)
}

/// [`open_regular`] sharing only `share` with other handles.
#[expect(
    clippy::disallowed_methods,
    reason = "opens the admin copy to read it, never to write; std's open is the one with flags"
)]
fn open_shared(path: &Path, share: u32) -> io::Result<File> {
    let file = File::options()
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let meta = file.metadata()?;
    if meta.file_attributes() & REPARSE_POINT != 0 || !meta.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// The SHA-256 of what `file` holds, as 64 lowercase hex digits.
fn hash_file(file: &mut File) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(super::hex(&hasher.finalize()))
}

/// Gives `path` (a folder when `folder`, else the admin copy) [`LOCKED_FOLDER_SDDL`]'s or
/// [`LOCKED_FILE_SDDL`]'s owner and protected DACL, then reads it back and refuses unless it is
/// exactly that ([`locked`]). Needs an elevated or SYSTEM token, which may assign
/// Administrators as owner.
pub(crate) fn lock_down(path: &Path, folder: bool) -> io::Result<()> {
    let sddl = if folder {
        LOCKED_FOLDER_SDDL
    } else {
        LOCKED_FILE_SDDL
    };
    let descriptor = Descriptor::from_sddl(sddl)?;
    let (owner, dacl) = descriptor.owner_and_dacl()?;
    let handle = Handle::open(path, READ_CONTROL.0 | WRITE_DAC.0 | WRITE_OWNER.0)?;
    // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER; the owner and DACL point into
    // `descriptor`, alive for the call. A folder's inheritable entries reach what is inside.
    let set = unsafe {
        SetSecurityInfo(
            handle.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            Some(owner),
            None,
            Some(dacl),
            None,
        )
    };
    if set != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(set.0).unwrap_or(i32::MAX),
        ));
    }
    let back = handle.security()?;
    if !locked(&back, folder) {
        return Err(io::Error::other(
            "the admin folder's permissions didn't read back as set",
        ));
    }
    Ok(())
}

/// The final path of this process's own image: `current_exe()` opened following every link on
/// the way, then `GetFinalPathNameByHandleW` (normalized, drive-letter form), so a junction or
/// link anywhere in the path gives the place the image really is.
pub(crate) fn running_image() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let target: Vec<u16> = exe.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `target` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(target.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(Handle)
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    let mut buffer = vec![0u16; 1024];
    loop {
        // SAFETY: the handle is open and `buffer` is writable for its length.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                handle.0,
                &mut buffer,
                GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
            )
        };
        match usize::try_from(length).unwrap_or(0) {
            0 => return Err(io::Error::last_os_error()),
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(PathBuf::from(OsString::from_wide(&buffer)));
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}

/// `path`'s owner and DACL as a self-relative descriptor, read from the name itself.
pub(crate) fn security(path: &Path) -> io::Result<Vec<u8>> {
    Handle::open(path, READ_CONTROL.0)?.security()
}

/// A string `SHGetKnownFolderPath` allocated, freed with `CoTaskMemFree` on drop.
struct TaskString(PWSTR);

impl Drop for TaskString {
    fn drop(&mut self) {
        // SAFETY: the pointer came from SHGetKnownFolderPath (or is null, which the call
        // accepts) and is freed once, here.
        unsafe { CoTaskMemFree(Some(self.0.0.cast_const().cast())) };
    }
}

/// A file or folder opened as itself; closed on drop.
struct Handle(HANDLE);

impl Handle {
    /// Opens `path` itself (`FILE_FLAG_OPEN_REPARSE_POINT`), a folder too, asking for `access`.
    fn open(path: &Path, access: u32) -> io::Result<Handle> {
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
        // SAFETY: `target` is NUL-terminated and outlives the call; no security attributes or
        // template are passed, and the handle is owned by `Handle` from here.
        unsafe {
            CreateFileW(
                PCWSTR(target.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .map(Handle)
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))
    }

    /// Its owner and DACL, self-relative (`GetKernelObjectSecurity`).
    fn security(&self) -> io::Result<Vec<u8>> {
        let info = (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0;
        let mut size = 256_u32;
        for _ in 0..4 {
            let mut buffer = vec![0_u64; usize::try_from(size).unwrap_or(0).div_ceil(8)];
            let mut needed = 0_u32;
            // SAFETY: the handle is open; `buffer` is writable for `size` bytes or more, and
            // `needed` is a live out-pointer.
            let read = unsafe {
                GetKernelObjectSecurity(
                    self.0,
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
                Err(e) => return Err(io::Error::from_raw_os_error(e.code().0 & 0xFFFF)),
            }
        }
        Err(io::Error::other(
            "a security descriptor kept growing while it was read",
        ))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once, here. A failed close
        // leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// A security descriptor allocated with `LocalAlloc`, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> io::Result<Descriptor> {
        let text: Vec<u16> = sddl.encode_utf16().chain(once(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
        // out-pointer, and the descriptor it receives is freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Descriptor(descriptor))
    }

    /// Its owner and DACL, which live as long as `self`.
    fn owner_and_dacl(&self) -> io::Result<(PSID, *const ACL)> {
        let mut owner = PSID::default();
        let mut owner_defaulted = BOOL(0);
        // SAFETY: the descriptor is valid while `self` lives; the out-pointers are live.
        unsafe { GetSecurityDescriptorOwner(self.0, &raw mut owner, &raw mut owner_defaulted) }
            .map_err(io::Error::other)?;
        let mut present = BOOL(0);
        let mut dacl: *mut ACL = null_mut();
        let mut defaulted = BOOL(0);
        // SAFETY: as above.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &raw mut present, &raw mut dacl, &raw mut defaulted)
        }
        .map_err(io::Error::other)?;
        if owner.is_invalid() || !present.as_bool() || dacl.is_null() {
            return Err(io::Error::other(
                "the admin folder's SDDL gave no owner or DACL",
            ));
        }
        Ok((owner, dacl.cast_const()))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc and freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}
