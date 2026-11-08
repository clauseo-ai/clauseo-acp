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
//!   sharing reads only, and the middle one closed. Opening that last one fails if any writer
//!   holds the file, and once it is open none can, nor rename or delete it. A writer that came
//!   and went in between changed bytes the check then reads: the check reads back through this
//!   last handle.
//! - **The start**: `ShellExecuteExW` "open" on the file with [`super::flow::ARGS`], waiting
//!   until the start is done (`SEE_MASK_NOASYNC`), on a thread with COM initialised as the shell
//!   asks. Not the updater plugin's install: that takes the bytes and writes its own copy to the
//!   temporary folder, runs it passive with `/UPDATE` (which skips the admin copy), and exits the
//!   process at once. The installer is `asInvoker`; its own elevated step raises the one UAC
//!   prompt. The handle is closed after the call returns.

#![expect(
    unsafe_code,
    reason = "FFI to CreateDirectoryW with a security descriptor, ReOpenFile and ShellExecuteExW"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "the attempt's folder: its version's folder made, and a failed attempt deleted; the \
              desktop crate has no write module"
)]

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
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
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

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
        std::fs::create_dir_all(&parent)?;
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
        let exe = HSTRING::from(self.exe.as_os_str());
        let folder = HSTRING::from(self.folder.as_os_str());
        let parameters = HSTRING::from(args.join(" "));
        // The shell wants COM on the calling thread: a thread of its own, joined.
        std::thread::scope(|scope| {
            scope
                .spawn(|| shell_open(&exe, &parameters, &folder))
                .join()
                .unwrap_or_else(|_panicked| Err(io::Error::other("the start panicked")))
        })
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

/// `ShellExecuteExW` "open" on `exe` with `parameters`, in `folder`, COM initialised for the
/// call. Returns once the start is done.
fn shell_open(exe: &HSTRING, parameters: &HSTRING, folder: &HSTRING) -> io::Result<()> {
    // SAFETY: no reserved pointer; balanced by CoUninitialize below when it succeeded.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: PCWSTR(folder.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points at are alive for the call.
    let started = unsafe { ShellExecuteExW(&raw mut info) }.map_err(io::Error::from);
    if com.is_ok() {
        // SAFETY: balances the CoInitializeEx above on this thread.
        unsafe { CoUninitialize() };
    }
    started
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
