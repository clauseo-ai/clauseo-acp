//! The stop's WSL step on the machine ([`VmStopper`] for [`WindowsStopper`]): HCS's compute
//! systems through `computecore.dll`, the profiles through `ProfileList`, the lawyer's
//! `Local AppData` redirect through its loaded hive (`HKU\<OwnerSid>`), and the services' keys.
//! `computecore.dll` is loaded at run time from System32 (`LoadLibraryExW`,
//! `LOAD_LIBRARY_SEARCH_SYSTEM32`) and its exports found by name, never imported: a machine without
//! it (no Hyper-V platform) must still start the exe, and has no VM to end. Every HCS call is
//! asynchronous: each gets an operation (`HcsCreateOperation`), waited for with a bounded timeout
//! (`HcsWaitForOperationResult`), one whose wait times out is cancelled (`HcsCancelOperation`,
//! where the DLL has it), and every operation and system handle is closed on drop. Errors name the
//! call and its code, never a path, an id or a SID.
#![expect(
    unsafe_code,
    reason = "FFI to computecore.dll, loaded at run time, and RegGetValueW on HKEY_USERS"
)]

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MOD_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, ERROR_TIMEOUT,
    FreeLibrary, GENERIC_ALL, HCS_E_OPERATION_TIMEOUT, HCS_E_SYSTEM_ALREADY_STOPPED,
    HCS_E_SYSTEM_NOT_FOUND, HLOCAL, HMODULE, LocalFree, WIN32_ERROR,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Registry::{
    HKEY_USERS, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::{HRESULT, HSTRING, PCSTR, PCWSTR, PWSTR, s, w};

use super::Failed;
use super::windows_stopper::WindowsStopper;
use super::{LIST_WAIT, VmId, VmStopper, local_app_data, wsl_ids_from_json};
use crate::windows::admin_copy::{Found, os};
use crate::windows::setup::profile::profile_of;
use crate::windows::sids::Sid;
use crate::windows::wsl::{LXSS_MANAGER, WSL_SERVICE, WindowsWsl, WslRead as _};

/// The Host Compute Service: with no service, no compute system runs.
const HCS_SERVICE: &str = "vmcompute";

/// An HCS handle (`HCS_OPERATION`, `HCS_SYSTEM`).
type Raw = *mut c_void;
/// `HcsEnumerateComputeSystems(query, operation)`.
type Enumerate = unsafe extern "system" fn(PCWSTR, Raw) -> HRESULT;
/// `HcsCreateOperation(context, callback)`; no callback: the operation is waited for.
type CreateOperation = unsafe extern "system" fn(*const c_void, *const c_void) -> Raw;
/// `HcsWaitForOperationResult(operation, timeoutMs, resultDocument)`.
type WaitForResult = unsafe extern "system" fn(Raw, u32, *mut PWSTR) -> HRESULT;
/// `HcsCancelOperation(operation)`.
type Cancel = unsafe extern "system" fn(Raw) -> HRESULT;
/// `HcsCloseOperation(operation)` and `HcsCloseComputeSystem(computeSystem)`.
type Close = unsafe extern "system" fn(Raw);
/// `HcsOpenComputeSystem(id, requestedAccess, computeSystem)`.
type Open = unsafe extern "system" fn(PCWSTR, u32, *mut Raw) -> HRESULT;
/// `HcsTerminateComputeSystem(computeSystem, operation, options)`.
type Terminate = unsafe extern "system" fn(Raw, Raw, PCWSTR) -> HRESULT;

/// Whether the service `name` is installed; a read that fails counts as installed.
fn service_installed(name: &str) -> bool {
    WindowsWsl::reader().service_exists(name).unwrap_or(true)
}

/// `call` failed with `code`, as a line for the log.
fn failed(call: &str, code: HRESULT) -> Failed {
    format!("{call} failed ({:#010x})", code.0)
}

/// Whether `code` says a wait for an operation ran out of time.
fn timed_out(code: HRESULT) -> bool {
    code == ERROR_TIMEOUT.to_hresult() || code == HCS_E_OPERATION_TIMEOUT
}

/// Whether `code` says the system is already gone.
fn gone(code: HRESULT) -> bool {
    code == HCS_E_SYSTEM_NOT_FOUND || code == HCS_E_SYSTEM_ALREADY_STOPPED
}

impl VmStopper for WindowsStopper {
    fn compute_systems(&mut self) -> Result<Option<Vec<String>>, Failed> {
        if !service_installed(HCS_SERVICE) {
            return Ok(None);
        }
        let Some(hcs) = Hcs::load()? else {
            return Ok(None);
        };
        wsl_ids_from_json(&hcs.list(LIST_WAIT)?).map(Some)
    }

    fn wsl_installed(&mut self) -> bool {
        [WSL_SERVICE, LXSS_MANAGER]
            .into_iter()
            .any(service_installed)
    }

    fn vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed> {
        let profile = profile_of(user)
            .map_err(|_error| "a pool user's ProfileList entry couldn't be read".to_owned())?;
        let Some(profile) = profile else {
            return Ok(false);
        };
        // The profile's folder, then each part below it, each read without following a link:
        // a pool user may have made one, to point the check at another user's folders.
        let mut at = profile.path;
        let parts = ["AppData", "Local", "temp", id.as_str()];
        for next in std::iter::once(None).chain(parts.iter().map(Some)) {
            if let Some(part) = next {
                at.push(part);
            }
            let last = next.is_some_and(|part| *part == id.as_str());
            match found(&at)? {
                Found::Missing => return Ok(false),
                Found::Folder => {}
                Found::File if last => return Ok(false),
                Found::File | Found::Link => {
                    return Err(
                        "a link, or a file where a folder belongs, is on the way to a \
                                pool user's VM folders"
                            .to_owned(),
                    );
                }
            }
        }
        Ok(true)
    }

    fn owner_vm_folder(&mut self, owner: &Sid, id: &VmId) -> Result<bool, Failed> {
        let profile = profile_of(owner)
            .map_err(|_error| "the lawyer's ProfileList entry couldn't be read".to_owned())?;
        let Some(profile) = profile else {
            return Ok(false);
        };
        // The lawyer's own folders: no pool user can write them, so links are followed.
        let mut default = profile.path.join(r"AppData\Local\temp");
        default.push(id.as_str());
        if owner_folder(&default)? {
            return Ok(true);
        }
        let Some(value) = redirect(owner)? else {
            return Ok(false);
        };
        let root = profile
            .path
            .to_str()
            .ok_or_else(|| "the lawyer's profile folder isn't text".to_owned())?;
        let local = local_app_data(&value, root)
            .ok_or_else(|| "the lawyer's Local AppData redirect can't be expanded".to_owned())?;
        let mut redirected = PathBuf::from(local);
        redirected.push("temp");
        redirected.push(id.as_str());
        owner_folder(&redirected)
    }

    fn terminate_vm(&mut self, id: &VmId, wait: Duration) -> Result<(), Failed> {
        let hcs = Hcs::load()?.ok_or_else(|| "computecore.dll is gone".to_owned())?;
        hcs.terminate(id, wait)
    }
}

/// Whether the lawyer's folder `at` is there, following links; nothing there is `false`.
fn owner_folder(at: &Path) -> Result<bool, Failed> {
    match std::fs::metadata(at) {
        Ok(found) => Ok(found.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(0);
            Err(format!("the lawyer's profile couldn't be read ({code})"))
        }
    }
}

/// `HKU\<owner>\...\Explorer\User Shell Folders`' `Local AppData`, unexpanded; `None` when it,
/// its key, or the loaded hive is missing.
fn redirect(owner: &Sid) -> Result<Option<String>, Failed> {
    let subkey = HSTRING::from(format!(
        r"{owner}\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders"
    ));
    let name = w!("Local AppData");
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let present = |code: WIN32_ERROR| match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(format!(
            "the lawyer's Local AppData redirect couldn't be read ({})",
            other.0
        )),
    };
    let mut size = 0u32;
    // SAFETY: NUL-terminated names; no data pointer asks only for the size.
    let code = unsafe {
        RegGetValueW(
            HKEY_USERS,
            &subkey,
            name,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if !present(code)? {
        return Ok(None);
    }
    let mut buffer = vec![0u16; usize::try_from(size).unwrap_or(0).div_ceil(2)];
    // SAFETY: `buffer` is live, writable and at least `size` bytes long for the call.
    let code = unsafe {
        RegGetValueW(
            HKEY_USERS,
            &subkey,
            name,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if !present(code)? {
        return Ok(None);
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let text = buffer.get(..end).unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_error| "the lawyer's Local AppData redirect isn't text".to_owned())
}

/// What is at `at`, never following a link there (its parents were each checked first).
fn found(at: &Path) -> Result<Found, Failed> {
    os::found(at).map_err(|error| {
        let code = error.raw_os_error().unwrap_or(0);
        format!("a pool user's profile couldn't be read ({code})")
    })
}

/// `computecore.dll`, loaded, with the exports the step calls; freed on drop, after them.
struct Hcs {
    enumerate: Enumerate,
    create_operation: CreateOperation,
    wait_for_result: WaitForResult,
    close_operation: Close,
    /// `HcsCancelOperation`: `None` where the DLL lacks it, and a timed-out operation is only
    /// closed.
    cancel: Option<Cancel>,
    open: Open,
    terminate: Terminate,
    close_system: Close,
    /// The module, freed when the exports can no longer be called.
    _library: Library,
}

/// The export `name` of `module` as the function pointer type `T`.
///
/// # Safety
///
/// `T` is the export's own signature, as a function pointer type.
unsafe fn export<T: Copy>(module: HMODULE, name: PCSTR) -> Result<T, Failed> {
    // SAFETY: a live module and a NUL-terminated name.
    let found = unsafe { GetProcAddress(module, name) };
    let found = found.ok_or_else(|| "computecore.dll lacks an export the stop needs".to_owned())?;
    assert_eq!(size_of::<T>(), size_of_val(&found), "a function pointer");
    // SAFETY: the caller's `T` is the export's signature, a function pointer of the same size.
    Ok(unsafe { std::mem::transmute_copy::<unsafe extern "system" fn() -> isize, T>(&found) })
}

/// The export `name` of `module` as `T`, or `None` when the module lacks it.
///
/// # Safety
///
/// As [`export`].
unsafe fn optional_export<T: Copy>(module: HMODULE, name: PCSTR) -> Option<T> {
    // SAFETY: as the caller's contract says.
    unsafe { export(module, name) }.ok()
}

impl Hcs {
    /// `computecore.dll` from System32 with its exports; `None` when it isn't there.
    fn load() -> Result<Option<Hcs>, Failed> {
        // SAFETY: a NUL-terminated name, searched for in System32 only.
        let loaded =
            unsafe { LoadLibraryExW(w!("computecore.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) };
        let module = match loaded {
            Ok(module) => module,
            Err(error) if error.code() == ERROR_MOD_NOT_FOUND.to_hresult() => return Ok(None),
            Err(error) => return Err(failed("LoadLibraryExW(computecore.dll)", error.code())),
        };
        let library = Library(module);
        // SAFETY: each type is the export's signature in computecore.h.
        let hcs = unsafe {
            Hcs {
                enumerate: export(module, s!("HcsEnumerateComputeSystems"))?,
                create_operation: export(module, s!("HcsCreateOperation"))?,
                wait_for_result: export(module, s!("HcsWaitForOperationResult"))?,
                close_operation: export(module, s!("HcsCloseOperation"))?,
                cancel: optional_export(module, s!("HcsCancelOperation")),
                open: export(module, s!("HcsOpenComputeSystem"))?,
                terminate: export(module, s!("HcsTerminateComputeSystem"))?,
                close_system: export(module, s!("HcsCloseComputeSystem"))?,
                _library: library,
            }
        };
        Ok(Some(hcs))
    }

    /// A new operation, closed on drop.
    fn operation(&self) -> Result<Operation<'_>, Failed> {
        // SAFETY: no context and no callback: the operation is waited for.
        let raw = unsafe { (self.create_operation)(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err("HcsCreateOperation failed".to_owned());
        }
        Ok(Operation(self, raw))
    }

    /// Waits up to `wait` for `operation`: its result, and its result document if it gave one.
    /// One that runs out of time is cancelled, where the DLL can, before it is closed on drop.
    fn wait(&self, operation: &Operation<'_>, wait: Duration) -> (HRESULT, Option<String>) {
        let wait_ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        let mut document = PWSTR::null();
        // SAFETY: a live operation, a bounded wait, and a live out-pointer.
        let code = unsafe { (self.wait_for_result)(operation.1, wait_ms, &raw mut document) };
        if timed_out(code)
            && let Some(cancel) = self.cancel
        {
            // SAFETY: a live operation, not yet closed.
            let cancelled = unsafe { cancel(operation.1) };
            if cancelled.is_err() {
                tracing::debug!(code = cancelled.0, "HcsCancelOperation failed");
            }
        }
        if document.is_null() {
            return (code, None);
        }
        // SAFETY: HCS wrote a NUL-terminated string.
        let text = unsafe { document.to_string() }.ok();
        // SAFETY: HCS allocated the document with LocalAlloc; freed once.
        unsafe { LocalFree(Some(HLOCAL(document.0.cast()))) };
        (code, text)
    }

    /// `HcsEnumerateComputeSystems` with an empty query (every system), waited for up to
    /// `wait`: its result document.
    fn list(&self, wait: Duration) -> Result<String, Failed> {
        let operation = self.operation()?;
        // SAFETY: a NUL-terminated query and a live operation.
        let started = unsafe { (self.enumerate)(w!("{}"), operation.1) };
        started
            .ok()
            .map_err(|_error| failed("HcsEnumerateComputeSystems", started))?;
        let (code, document) = self.wait(&operation, wait);
        code.ok()
            .map_err(|_error| failed("HcsWaitForOperationResult (the list)", code))?;
        document.ok_or_else(|| "HCS's list of compute systems came back empty".to_owned())
    }

    /// Opens the system `id` and terminates it, waiting up to `wait`; one already gone is `Ok`.
    fn terminate(&self, id: &VmId, wait: Duration) -> Result<(), Failed> {
        let name = HSTRING::from(id.as_str());
        let mut raw: Raw = ptr::null_mut();
        // SAFETY: a NUL-terminated id and a live out-pointer, owned by `System` once written.
        let opened = unsafe { (self.open)(PCWSTR(name.as_ptr()), GENERIC_ALL.0, &raw mut raw) };
        if gone(opened) {
            return Ok(());
        }
        opened
            .ok()
            .map_err(|_error| failed("HcsOpenComputeSystem", opened))?;
        let system = System(self, raw);
        let operation = self.operation()?;
        // SAFETY: a live system and a live operation; no options.
        let started = unsafe { (self.terminate)(system.1, operation.1, PCWSTR::null()) };
        if gone(started) {
            return Ok(());
        }
        started
            .ok()
            .map_err(|_error| failed("HcsTerminateComputeSystem", started))?;
        let (code, _document) = self.wait(&operation, wait);
        if code.is_ok() || gone(code) {
            Ok(())
        } else {
            Err(failed("HcsWaitForOperationResult (the termination)", code))
        }
    }
}

/// A loaded module, freed on drop.
struct Library(HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: the module `Hcs::load` loaded, freed once; no export of it is called after.
        if let Err(error) = unsafe { FreeLibrary(self.0) } {
            tracing::debug!(code = error.code().0, "FreeLibrary(computecore.dll) failed");
        }
    }
}

/// An HCS operation, closed on drop.
struct Operation<'a>(&'a Hcs, Raw);

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        // SAFETY: the operation HcsCreateOperation gave, closed once.
        unsafe { (self.0.close_operation)(self.1) };
    }
}

/// An open compute system, closed on drop.
struct System<'a>(&'a Hcs, Raw);

impl Drop for System<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle HcsOpenComputeSystem gave, closed once.
        unsafe { (self.0.close_system)(self.1) };
    }
}
