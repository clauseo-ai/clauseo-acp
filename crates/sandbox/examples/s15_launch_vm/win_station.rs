//! The harness's control station: a fresh window station with a `Default` desktop, kept open
//! while a control process runs on it, made the unelevated way the outer runner makes the
//! agent's (`src/windows/runner/station.rs`). An unelevated user can't make a named station
//! (`\Sessions\<n>\Windows\WindowStations` grants `CreateChild` to `Administrators` alone: 5),
//! so the harness logs a pool user on (`LogonUserW`, interactive), makes an unnamed station while
//! acting as that logon (Windows names it `Service-0x0-<logon LUID>$`, one per logon, so two
//! control stations never meet), goes back to itself, opens the station by its name, makes
//! itself its owner, makes `Default` on it and sets the last DACL. The logon is the maker's
//! only: the control process may run as anyone the DACL names.
#![expect(
    unsafe_code,
    reason = "FFI to the logon, window station, desktop and security descriptor calls"
)]

use std::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_WINDOW_OBJECT,
    SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
    ImpersonateLoggedOnUser, LABEL_SECURITY_INFORMATION, LOGON32_LOGON_INTERACTIVE,
    LOGON32_PROVIDER_DEFAULT, LogonUserW, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW, DESKTOP_CONTROL_FLAGS,
    GetProcessWindowStation, GetUserObjectInformationW, HDESK, HWINSTA, OpenWindowStationW,
    SetProcessWindowStation, UOI_NAME,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};
use zeroize::Zeroizing;

use super::win::{Owned, code_of};

/// A failure: the call and its Win32 code.
type Failed = (&'static str, u32);

/// What the maker's logon asks for on the station it makes, and what the creation DACL grants
/// `OWNER RIGHTS` (the maker owns it then): `READ_CONTROL` alone, as the runner's.
const MAKE_RIGHTS: u32 = 0x0002_0000;
/// What the harness asks for when it opens the station, and what the creation DACL grants it:
/// `READ_CONTROL | WRITE_DAC | WRITE_OWNER | WINSTA_CREATEDESKTOP`.
const OPEN_RIGHTS: u32 = 0x000E_0008;
/// `GENERIC_ALL`, what the harness asks for on the desktop (its DACL grants it `GA`).
const GENERIC_ALL: u32 = 0x1000_0000;

/// The control station and its desktop, closed on drop, the desktop first.
pub(crate) struct ControlStation {
    _desktop: Desktop,
    _station: Station,
    /// The station's name, `Service-0x0-<logon LUID>$`.
    name: String,
}

impl ControlStation {
    /// Makes the station through the local user `maker`'s logon (with `password`), and its
    /// desktop, each with a protected DACL of `SYSTEM` and `sids` (`GA` each), labelled Low. The
    /// call that failed and its code otherwise.
    pub(crate) fn open(
        maker: &str,
        password: &Zeroizing<String>,
        sids: &[&str],
    ) -> Result<ControlStation, Failed> {
        let me = super::win::own_token()
            .map_err(|e| {
                (
                    "OpenProcessToken",
                    e.raw_os_error().map_or(0, i32::cast_unsigned),
                )
            })?
            .user;
        let made = Descriptor::new(&format!(
            "D:P(A;;0xF037F;;;SY)(A;;0x{OPEN_RIGHTS:X};;;{me})(A;;0x{MAKE_RIGHTS:X};;;OW)"
        ))?;
        let aces = sids.iter().fold(String::new(), |mut aces, sid| {
            for part in ["(A;;GA;;;", sid, ")"] {
                aces.push_str(part);
            }
            aces
        });
        let last = Descriptor::new(&format!("D:P(A;;GA;;;SY){aces}S:(ML;;NW;;;LW)"))?;
        let logon = log_on(maker, password)?;
        let created = create_as(&logon, &made);
        drop(logon);
        let created = Station(created?);
        let name = name_of(created.0)?;
        // SAFETY: the name is a live HSTRING; the handle is owned by `Station` at once.
        let station = Station(
            unsafe { OpenWindowStationW(&HSTRING::from(name.as_str()), false, OPEN_RIGHTS) }
                .map_err(|e| ("OpenWindowStationW", code_of(&e)))?,
        );
        drop(created);
        let owner = Descriptor::new(&format!("O:{me}"))?;
        // SAFETY: the handle is live with WRITE_OWNER; the SID lives in `owner`.
        let set = unsafe {
            SetSecurityInfo(
                HANDLE(station.0.0),
                SE_WINDOW_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(owner.owner()?),
                None,
                None,
                None,
            )
        };
        win32("SetSecurityInfo (owner)", set.0)?;
        let desktop = desktop_on(station.0, &last)?;
        secure(HANDLE(station.0.0), &last)?;
        Ok(ControlStation {
            _desktop: desktop,
            _station: station,
            name,
        })
    }

    /// `<station>\Default`, for a process's `lpDesktop`.
    pub(crate) fn desktop(&self) -> String {
        format!(r"{}\Default", self.name)
    }
}

/// A station handle, closed on drop.
struct Station(HWINSTA);

/// A desktop handle, closed on drop.
struct Desktop(HDESK);

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY: a live desktop handle, closed once, here.
        drop(unsafe { CloseDesktop(self.0) });
    }
}

impl Drop for Station {
    fn drop(&mut self) {
        // SAFETY: a live station handle, closed once, here.
        drop(unsafe { CloseWindowStation(self.0) });
    }
}

/// `maker`'s interactive logon token; the password's UTF-16 copy is wiped when this returns.
fn log_on(maker: &str, password: &Zeroizing<String>) -> Result<Owned, Failed> {
    let units = password.encode_utf16().count();
    let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(units + 1));
    secret.extend(password.encode_utf16());
    secret.push(0);
    let mut token = HANDLE::default();
    // SAFETY: the strings are NUL-terminated and outlive the call; `token` is a live out-pointer,
    // owned by `Owned` once written.
    let logged_on = unsafe {
        LogonUserW(
            &HSTRING::from(maker),
            w!("."),
            PCWSTR(secret.as_ptr()),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &raw mut token,
        )
    };
    drop(secret);
    logged_on.map_err(|e| ("LogonUserW", code_of(&e)))?;
    Ok(Owned(token))
}

/// The unnamed station, made while this thread acts as `logon`; the thread is itself again on
/// every path out ([`Reverting`]).
fn create_as(logon: &Owned, made: &Descriptor) -> Result<HWINSTA, Failed> {
    let _reverting = Reverting;
    // SAFETY: a live logon token with every right (LogonUserW's).
    unsafe { ImpersonateLoggedOnUser(logon.0) }
        .map_err(|e| ("ImpersonateLoggedOnUser", code_of(&e)))?;
    let attributes = made.attributes();
    // SAFETY: no name; the attributes and their descriptor outlive the call.
    unsafe { CreateWindowStationW(PCWSTR::null(), 0, MAKE_RIGHTS, Some(&raw const attributes)) }
        .map_err(|e| ("CreateWindowStationW", code_of(&e)))
}

/// Reverts this thread on drop; a thread Windows won't revert ends the process, since nothing is
/// safe to run on it.
struct Reverting;

impl Drop for Reverting {
    fn drop(&mut self) {
        // SAFETY: no arguments; it ends whatever impersonation the calling thread has.
        if unsafe { RevertToSelf() }.is_err() {
            std::process::abort();
        }
    }
}

/// The station's name (`UOI_NAME`).
fn name_of(station: HWINSTA) -> Result<String, Failed> {
    let mut buffer = [0u16; 256];
    let len = u32::try_from(size_of_val(&buffer)).unwrap_or(0);
    // SAFETY: the handle is live; `buffer` is writable for `len` bytes.
    unsafe {
        GetUserObjectInformationW(
            HANDLE(station.0),
            UOI_NAME,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            len,
            None,
        )
    }
    .map_err(|e| ("GetUserObjectInformationW", code_of(&e)))?;
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(
        buffer.get(..end).unwrap_or_default(),
    ))
}

/// Makes the desktop `Default` on `station` with `descriptor`, this process switched to it for
/// the call only; its handle, open.
fn desktop_on(station: HWINSTA, descriptor: &Descriptor) -> Result<Desktop, Failed> {
    let attributes = descriptor.attributes();
    // SAFETY: plain call; the process's own station isn't ours to close.
    let own = unsafe { GetProcessWindowStation() }
        .map_err(|e| ("GetProcessWindowStation", code_of(&e)))?;
    // SAFETY: `station` is a live window station handle with WINSTA_CREATEDESKTOP.
    unsafe { SetProcessWindowStation(station) }
        .map_err(|e| ("SetProcessWindowStation", code_of(&e)))?;
    // SAFETY: the name is static; `attributes` and its descriptor outlive the call.
    let desktop = unsafe {
        CreateDesktopW(
            w!("Default"),
            PCWSTR::null(),
            None,
            DESKTOP_CONTROL_FLAGS::default(),
            GENERIC_ALL,
            Some(&raw const attributes),
        )
    }
    .map(Desktop)
    .map_err(|e| ("CreateDesktopW", code_of(&e)));
    // SAFETY: `own` is this process's own station, still open.
    let back = unsafe { SetProcessWindowStation(own) };
    let desktop = desktop?;
    back.map_err(|e| ("SetProcessWindowStation (back)", code_of(&e)))?;
    Ok(desktop)
}

/// Gives the window object `handle` the protected DACL and the label of `descriptor`.
fn secure(handle: HANDLE, descriptor: &Descriptor) -> Result<(), Failed> {
    let call = "SetSecurityInfo (DACL and label)";
    let (dacl, sacl) = (descriptor.dacl()?, descriptor.sacl()?);
    // SAFETY: the handle is live with WRITE_DAC and WRITE_OWNER; the ACLs live in `descriptor`.
    let set = unsafe {
        SetSecurityInfo(
            handle,
            SE_WINDOW_OBJECT,
            DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION
                | LABEL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl.cast_const()),
            Some(sacl.cast_const()),
        )
    };
    win32(call, set.0)
}

/// A `SetSecurityInfo` result as a failure of `call`.
fn win32(call: &'static str, code: u32) -> Result<(), Failed> {
    if code == 0 { Ok(()) } else { Err((call, code)) }
}

/// A security descriptor made from SDDL, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn new(sddl: &str) -> Result<Descriptor, Failed> {
        let mut out = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the text is a live HSTRING; `out` is a live out-pointer, freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &raw mut out,
                None,
            )
        }
        .map_err(|e| {
            (
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                code_of(&e),
            )
        })?;
        Ok(Descriptor(out))
    }

    /// Security attributes holding it, not inherited; valid while `self` lives.
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }

    fn dacl(&self) -> Result<*mut ACL, Failed> {
        let (mut present, mut defaulted, mut acl) = (BOOL(0), BOOL(0), std::ptr::null_mut());
        // SAFETY: the descriptor lives while `self` does; the out-pointers are live.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &raw mut present, &raw mut acl, &raw mut defaulted)
        }
        .map_err(|e| ("GetSecurityDescriptorDacl", code_of(&e)))?;
        Ok(acl)
    }

    fn sacl(&self) -> Result<*mut ACL, Failed> {
        let (mut present, mut defaulted, mut acl) = (BOOL(0), BOOL(0), std::ptr::null_mut());
        // SAFETY: as in `dacl`.
        unsafe {
            GetSecurityDescriptorSacl(self.0, &raw mut present, &raw mut acl, &raw mut defaulted)
        }
        .map_err(|e| ("GetSecurityDescriptorSacl", code_of(&e)))?;
        Ok(acl)
    }

    fn owner(&self) -> Result<PSID, Failed> {
        let (mut owner, mut defaulted) = (PSID::default(), BOOL(0));
        // SAFETY: as in `dacl`.
        unsafe {
            windows::Win32::Security::GetSecurityDescriptorOwner(
                self.0,
                &raw mut owner,
                &raw mut defaulted,
            )
        }
        .map_err(|e| ("GetSecurityDescriptorOwner", code_of(&e)))?;
        Ok(owner)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion with LocalAlloc, freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}
