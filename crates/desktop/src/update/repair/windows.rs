//! The attempt on Windows: its folder, its file and the installer's start.
//!
//! - **The folder**, `updates/<version>/repair-<uuid>`, is created with its DACL in the same
//!   call (`CreateDirectoryW` with `SECURITY_ATTRIBUTES`): protected, so nothing is inherited
//!   from `updates/`, and full control for this process's user and SYSTEM alone, both
//!   inheritable, so the file gets the same.
//! - **The file** is created new in it (`CREATE_NEW`) for reading and writing, sharing reads
//!   only: no other handle may write, rename or delete it. The download is written through it.
//! - **The seal.** Windows will not start an image while a handle holds it for writing (the
//!   loader opens it without sharing writes), so writing ends with two reopenings of that handle,
//!   never of the path (`ReOpenFile`): a read handle sharing reads and writes (its share mode
//!   must admit the write handle still open), the write handle closed, then a read handle
//!   sharing reads only, and the middle one closed. Opening that last one fails if a writer
//!   handle is still open, and once it is open no new writer handle, rename or delete gets in.
//!   It does not undo what the middle window let in: a process of the same user can open the
//!   file for writing there, map it writable and close its handle, and that mapping survives the
//!   seal. What catches altered bytes is the read-back through this last handle, which the check
//!   verifies; a change after it is the same-user residual decision 0049 accepts ("What stays
//!   open"), as is the gap before the installer opens its own image.
//! - **The lock-down** (`lockdown.rs`), once sealed: the folder becomes read and execute for
//!   the user, so nothing can be planted beside the installer.
//! - **The start**: `CreateProcessW` with the sealed path as the application name and
//!   `"<path>" /CLAUSEO_REPAIR` as the command line: no shell, so no file-type lookup (an
//!   `HKCU\Software\Classes\exefile` override can't redirect it), no search path, and no handle
//!   inherited, started in `%SystemRoot%\System32`. The installer is `asInvoker`; its own elevated
//!   step raises the one UAC prompt. One that asks to start elevated (`ERROR_ELEVATION_REQUIRED`)
//!   is a failed launch, never retried through the shell. Not the updater plugin's install: that
//!   takes the bytes and writes its own copy to the temporary folder, runs it passive with
//!   `/UPDATE` (which skips the admin copy), and exits the process at once. The handle is closed
//!   after the call returns.

#![expect(
    unsafe_code,
    reason = "FFI to CreateDirectoryW with a security descriptor, ReOpenFile and CreateProcessW"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "the attempt's folder: its version's folder made, and a failed attempt deleted; the \
              desktop crate has no write module"
)]

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ELEVATION_REQUIRED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, ReOpenFile,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, OpenProcessToken, PROCESS_CREATION_FLAGS,
    PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::flow::Staged;
use super::sweep::PREFIX;

/// One attempt's folder and file, and the handle that holds the file.
pub(super) struct Stage {
    folder: PathBuf,
    exe: PathBuf,
    held: Option<File>,
}

impl Stage {
    /// A new attempt for `version` under `updates`: the folder, with its DACL, and `file` in it,
    /// created new.
    pub(super) fn create(updates: &Path, version: &str, file: &str) -> io::Result<Self> {
        let parent = updates.join(version);
        // `updates/` and its version's folder: never a link or junction, before and after they
        // are made, as the sweep reads them.
        refuse_reparse(&[updates, &parent])?;
        std::fs::create_dir_all(&parent)?;
        refuse_reparse(&[updates, &parent])?;
        let folder = parent.join(format!("{PREFIX}{}", uuid::Uuid::now_v7().simple()));
        let descriptor = OwnerOnly::new()?;
        let attributes = descriptor.attributes();
        // SAFETY: the path is a live HSTRING; `attributes` points at a descriptor alive until
        // `descriptor` drops, after the call.
        unsafe {
            CreateDirectoryW(
                &HSTRING::from(folder.as_os_str()),
                Some(&raw const attributes),
            )
        }
        .map_err(io::Error::from)?;
        let exe = folder.join(file);
        let created = create_new(&exe);
        match created {
            Ok(held) => Ok(Self {
                folder,
                exe,
                held: Some(held),
            }),
            Err(error) => {
                drop(std::fs::remove_dir(&folder));
                Err(error)
            }
        }
    }

    /// Starts the installer with `args` through the shell, the handle held throughout.
    pub(super) fn launch(&self, args: &[&str]) -> io::Result<()> {
        if self.held.is_none() {
            return Err(io::Error::other("the installer is not held"));
        }
        let started = create_process(&self.exe, args);
        if let Err(error) = &started
            && error.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED.0.cast_signed())
        {
            tracing::warn!(
                target: super::super::install::LIFECYCLE,
                "repair: the installer asked to start elevated; not started"
            );
        }
        started
    }

    /// Closes the handle and deletes the folder. The folder is ours and fresh: no link in it.
    pub(super) fn discard(mut self) {
        drop(self.held.take());
        if let Err(error) = std::fs::remove_dir_all(&self.folder) {
            tracing::warn!(
                target: super::super::install::LIFECYCLE,
                kind = %error.kind(),
                "repair: the attempt's folder was not deleted"
            );
        }
    }

    /// The installer's path, for the tests.
    #[cfg(test)]
    pub(super) fn exe(&self) -> &Path {
        &self.exe
    }

    /// The attempt's folder, for the tests.
    #[cfg(test)]
    pub(super) fn folder(&self) -> &Path {
        &self.folder
    }

    fn held(&mut self) -> io::Result<&mut File> {
        self.held
            .as_mut()
            .ok_or_else(|| io::Error::other("the installer is not held"))
    }
}

impl Staged for Stage {
    fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        self.held()?.write_all(chunk)
    }

    fn seal(&mut self) -> io::Result<()> {
        let writer = self
            .held
            .take()
            .ok_or_else(|| io::Error::other("the installer is not held"))?;
        writer.sync_all()?;
        let shared = reopen(&writer, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        drop(writer);
        let held = reopen(&shared, FILE_SHARE_READ)?;
        drop(shared);
        self.held = Some(held);
        // Written and sealed: nothing may be added beside it now (`lockdown.rs`), and nothing
        // added before may stay.
        super::lockdown::lock_down(&self.folder)?;
        let file = self
            .exe
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !super::plan::only_installer(&super::lockdown::entries(&self.folder)?, file) {
            tracing::warn!(
                target: super::super::install::LIFECYCLE,
                "repair: the attempt's folder holds more than its installer; refused"
            );
            return Err(io::Error::other(
                "the attempt's folder holds more than its installer",
            ));
        }
        Ok(())
    }

    fn read_back(&mut self) -> io::Result<Vec<u8>> {
        let held = self.held()?;
        held.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        held.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Refuses when any of `folders` that exists is a link or a reparse point (a junction
/// included) or not a folder.
fn refuse_reparse(folders: &[&Path]) -> io::Result<()> {
    for folder in folders {
        match std::fs::symlink_metadata(folder) {
            Ok(meta) if !super::sweep::plain_folder(&meta) => {
                return Err(io::Error::other("a link or junction in the updates folder"));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// `path`, created new for reading and writing, sharing reads only; its DACL inherited from the
/// folder's.
fn create_new(path: &Path) -> io::Result<File> {
    // SAFETY: a live HSTRING; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(path.as_os_str()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ,
            None,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(io::Error::from)?;
    Ok(owned(handle))
}

/// A read handle on the file `file` holds, sharing `share`: from the handle, never the path.
fn reopen(file: &File, share: FILE_SHARE_MODE) -> io::Result<File> {
    let original = HANDLE(file.as_raw_handle());
    // SAFETY: `original` is a live handle `file` owns for the call.
    let handle = unsafe {
        ReOpenFile(
            original,
            GENERIC_READ.0,
            share,
            FILE_FLAGS_AND_ATTRIBUTES(0),
        )
    }
    .map_err(io::Error::from)?;
    Ok(owned(handle))
}

/// `handle` as an owned `File`, closed when it drops.
fn owned(handle: HANDLE) -> File {
    // SAFETY: a fresh handle from CreateFileW or ReOpenFile that nothing else owns.
    unsafe { File::from_raw_handle(handle.0) }
}

/// `CreateProcessW` on `exe` (the application name, the verified path: no search, no shell)
/// with the command line `"<exe>" <args>`, inheriting no handle, its working folder
/// `%SystemRoot%\System32` (`GetSystemDirectoryW`) rather than the attempt's, so nothing it
/// loads by a relative name comes from beside it. Returns once the process exists; its handles
/// are closed at once, as nothing waits for it.
fn create_process(exe: &Path, args: &[&str]) -> io::Result<()> {
    let application = HSTRING::from(exe.as_os_str());
    let directory = HSTRING::from_wide(&system_directory()?);
    // CreateProcessW may write to the command line: a buffer of our own, NUL-terminated.
    let mut line: Vec<u16> = std::iter::once(u16::from(b'"'))
        .chain(exe.as_os_str().encode_wide())
        .chain(std::iter::once(u16::from(b'"')))
        .chain(
            args.iter()
                .flat_map(|arg| std::iter::once(u16::from(b' ')).chain(arg.encode_utf16())),
        )
        .chain(std::iter::once(0))
        .collect();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: the strings and the command line buffer are alive and NUL-terminated for the call;
    // `startup` and `process` are live; no handle is inherited.
    unsafe {
        CreateProcessW(
            &application,
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            &directory,
            &raw const startup,
            &raw mut process,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: both handles came from CreateProcessW above and are closed once.
    unsafe {
        drop(CloseHandle(process.hThread));
        drop(CloseHandle(process.hProcess));
    }
    Ok(())
}

/// `%SystemRoot%\System32`, as `GetSystemDirectoryW` gives it (no environment variable read).
fn system_directory() -> io::Result<Vec<u16>> {
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: `buffer` is a live, writable slice for the call.
        let written = unsafe { GetSystemDirectoryW(Some(&mut buffer)) };
        let written = usize::try_from(written).unwrap_or(usize::MAX);
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if written < buffer.len() {
            buffer.truncate(written);
            return Ok(buffer);
        }
        // Too small: `written` is the size wanted, its NUL included.
        buffer.resize(written, 0);
    }
}

/// Whether a window of Clauseo's main window class (`lib.rs`'s `MAIN_WINDOW_CLASS`) exists in
/// this session, hidden ones included: the installer refuses while one does.
pub(super) fn main_window_class_exists() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    let class = HSTRING::from(crate::MAIN_WINDOW_CLASS);
    // SAFETY: a live HSTRING; no window name.
    unsafe { FindWindowW(&class, PCWSTR::null()) }.is_ok_and(|window| !window.is_invalid())
}

/// A security descriptor whose DACL is protected and gives full control, inherited by files and
/// folders under it, to this process's user and SYSTEM alone.
struct OwnerOnly(PSECURITY_DESCRIPTOR);

impl OwnerOnly {
    fn new() -> io::Result<Self> {
        let sid = own_sid()?;
        let sddl = HSTRING::from(format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a live string and out-pointer; freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(io::Error::from)?;
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// This process's user SID, as `ConvertSidToStringSidW` spells it.
pub(super) fn own_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle needs no close; `token` is a live out-pointer, closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
        .map_err(io::Error::from)?;
    let sid = token_user_sid(token);
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(token) });
    sid
}

/// The user SID of `token`, spelled out.
fn token_user_sid(token: HANDLE) -> io::Result<String> {
    let mut needed = 0u32;
    // SAFETY: a size query: no buffer, a live out-pointer. It fails with the size wanted.
    drop(unsafe { GetTokenInformation(token, TokenUser, None, 0, &raw mut needed) });
    let words = usize::try_from(needed)
        .unwrap_or(0)
        .div_ceil(size_of::<u64>());
    if words == 0 {
        return Err(io::Error::other("the token's user was not read"));
    }
    // u64s, so the TOKEN_USER at its start is aligned.
    let mut buffer = vec![0u64; words];
    // SAFETY: `buffer` holds at least `needed` bytes for the call.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            needed,
            &raw mut needed,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of the aligned buffer, which
    // outlives `user`.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID is valid while `buffer` lives; `text` is freed below.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) }.map_err(io::Error::from)?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let sid = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid
}
