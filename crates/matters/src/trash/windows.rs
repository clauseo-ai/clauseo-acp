//! Windows: the Recycle Bin, through the shell's `IFileOperation`, never showing UI.
//!
//! - Flags: `FOF_NO_UI` (no progress, no confirmation, no error dialog), `FOF_ALLOWUNDO` and
//!   `FOFX_RECYCLEONDELETE` (to the Recycle Bin), `FOFX_EARLYFAILURE` (stop at the first error).
//!   No `FOF_WANTNUKEWARNING`: that asks the lawyer.
//! - No Recycle Bin: under `FOF_NO_UI` the shell deletes for good, silently, a file it cannot
//!   recycle (a share, a removable or exFAT drive, a bin turned off or too small). So only a
//!   fixed drive is tried (`GetDriveTypeW`); and on it, the progress sink's `PreDeleteItem`
//!   refuses any delete whose flags lack `TSF_DELETE_RECYCLE_IF_POSSIBLE`, which cancels the
//!   operation. Either way the answer is [`TrashError::NoTrash`] and writeback's fallback.
//! - COM: initialised on the calling thread (the matter worker's) as single-threaded for the
//!   call and undone after; a thread already in the multithreaded apartment
//!   (`RPC_E_CHANGED_MODE`) keeps it, and the call goes on there.
//! - Long paths: the shell takes no `\\?\` path. A verbatim drive path is passed in its plain
//!   form when that is under 260 UTF-16 units; a longer one, or a verbatim UNC or volume path,
//!   is [`TrashError::NoTrash`].
#![expect(unsafe_code, reason = "COM calls to the shell's IFileOperation")]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NO_UI, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
    IFileOperation, IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING};
use windows_core::ComObject;

use super::TrashError;
use crate::long_path::plain;
use sink::Sink;

mod sink;

/// `GetDriveTypeW`'s answer for a fixed disk (`winbase.h`).
const DRIVE_FIXED: u32 = 3;
/// The Win32 path limit the shell keeps (`MAX_PATH`), its NUL included.
const MAX_PATH: usize = 260;
/// `\\?\`, the verbatim prefix.
const VERBATIM: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];

/// Moves `path` to the Recycle Bin; returns where the bin keeps it.
pub(super) fn trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    let shell_path = shell_path(path).ok_or(TrashError::NoTrash)?;
    if crate::place::drive_type(path) != Some(DRIVE_FIXED) {
        return Err(TrashError::NoTrash);
    }
    let _com = Com::init();
    recycle(&shell_path)
}

/// `path` as the shell can parse it: plain, under [`MAX_PATH`]; `None` otherwise.
fn shell_path(path: &Path) -> Option<HSTRING> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let wide = plain(&wide);
    if wide.starts_with(&VERBATIM) || wide.len() >= MAX_PATH {
        return None;
    }
    Some(HSTRING::from_wide(&wide))
}

/// COM on this thread for one call: undone on drop when this call started it.
struct Com {
    started: bool,
}

impl Com {
    fn init() -> Self {
        // SAFETY: no reserved pointer. S_OK and S_FALSE both count once, so both are undone;
        // RPC_E_CHANGED_MODE (the thread is in the multithreaded apartment) leaves it as it is.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result.is_err() && result != RPC_E_CHANGED_MODE {
            tracing::debug!(
                code = result.0,
                "CoInitializeEx failed; trying the call anyway"
            );
        }
        Self {
            started: result.is_ok(),
        }
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.started {
            // SAFETY: balances this call's successful CoInitializeEx, on the same thread, after
            // every COM object of the call was released (`recycle` returned).
            unsafe { CoUninitialize() };
        }
    }
}

/// The recycle: one `IFileOperation` with the sink. Every COM object is released on return.
fn recycle(shell_path: &HSTRING) -> Result<Option<PathBuf>, TrashError> {
    let failed = |err: windows::core::Error| TrashError::Failed(io_error(err.code()));
    // SAFETY: COM is initialised on this thread (or the thread is in the MTA); the class id is
    // the shell's FileOperation, created in process with no outer object.
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(failed)?;
    // SAFETY: a live IFileOperation; the flags are constants.
    unsafe {
        operation
            .SetOperationFlags(FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE)
    }
    .map_err(failed)?;
    // SAFETY: `shell_path` is a NUL-terminated UTF-16 string that outlives the call; no bind
    // context.
    let item: IShellItem =
        unsafe { SHCreateItemFromParsingName(shell_path, None) }.map_err(failed)?;
    let sink = ComObject::new(Sink::default());
    let progress: IFileOperationProgressSink = sink.to_interface();
    // SAFETY: live interfaces; the operation keeps its own references to both.
    unsafe { operation.DeleteItem(&item, &progress) }.map_err(failed)?;
    // SAFETY: a live IFileOperation with one delete queued.
    let performed = unsafe { operation.PerformOperations() };
    let seen = sink.with_seen(std::mem::take);
    if seen.refused {
        return Err(TrashError::NoTrash);
    }
    performed.map_err(failed)?;
    // SAFETY: a live IFileOperation after PerformOperations.
    let aborted = unsafe { operation.GetAnyOperationsAborted() }.map_err(failed)?;
    match seen.result {
        Some(result) if result.is_err() => Err(TrashError::Failed(io_error(result))),
        Some(_) if !aborted.as_bool() => Ok(seen.recycled),
        Some(_) | None => Err(TrashError::Failed(io::ErrorKind::Interrupted.into())),
    }
}

/// `code` as an I/O error: a Win32 error inside an HRESULT (`0x8007xxxx`) as that error, so
/// a missing file is `NotFound`; any other as the HRESULT.
fn io_error(code: HRESULT) -> io::Error {
    // The bits as the OS returns them; HRESULT is a signed view of them.
    let bits = code.0.cast_unsigned();
    if bits & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((bits & 0xFFFF).cast_signed())
    } else {
        io::Error::other(format!("HRESULT {bits:#010x}"))
    }
}

/// Where the shell item `item` is on disk, if it has a file system path.
fn file_path(item: &IShellItem) -> Option<PathBuf> {
    // SAFETY: a live IShellItem; the name it returns is ours to free with CoTaskMemFree.
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
    // SAFETY: `name` is a NUL-terminated UTF-16 string the call allocated.
    let path = unsafe { name.to_string() }.ok().map(PathBuf::from);
    // SAFETY: frees the string GetDisplayName allocated, once.
    unsafe { CoTaskMemFree(Some(name.as_ptr().cast_const().cast())) };
    path
}
