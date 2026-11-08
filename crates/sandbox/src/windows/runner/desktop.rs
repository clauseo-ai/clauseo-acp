//! The agent's window station and desktop (Windows), made by the outer runner (the desktop
//! user, unelevated) before the inner runner joins the job, so the job can carry the desktop UI
//! limit. The order and why are `station.rs`'s; these are its calls.
//!
//! The (B) token passes both objects' checks through the pool user's SID, a restricting SID too.
//! Neither the lawyer's `WinSta0` (whose DACL names the logon SID, deny-only in the token) nor
//! the service window station a runner without a desktop session gets grants the token anything:
//! user32 then fails to start the agent (`0xC0000142`, `STATUS_DLL_INIT_FAILED`; the first
//! Windows runs of the harness). A station of its own also keeps the agent's clipboard and
//! global atoms apart from the lawyer's, whatever the job's UI limits.
#![expect(
    unsafe_code,
    reason = "FFI to the logon, window station and desktop calls"
)]

use std::ffi::c_void;
use std::io::{self, Write as _};

use windows::Wdk::Foundation::{NtQueryObject, OBJECT_INFORMATION_CLASS, OBJECT_NAME_INFORMATION};
use windows::Win32::Foundation::{GetLastError, HANDLE, SetLastError, WIN32_ERROR};
use windows::Win32::Security::Authorization::{SE_WINDOW_OBJECT, SetSecurityInfo};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetTokenInformation, ImpersonateLoggedOnUser,
    LABEL_SECURITY_INFORMATION, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW,
    OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    RevertToSelf, SECURITY_ATTRIBUTES, SECURITY_IMPERSONATION_LEVEL, SecurityImpersonation,
    TOKEN_QUERY, TokenImpersonationLevel,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW, DESKTOP_CONTROL_FLAGS,
    GetProcessWindowStation, HDESK, HWINSTA, OpenWindowStationW, SetProcessWindowStation,
};
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
use windows::core::{HSTRING, PCWSTR, w};
use zeroize::Zeroizing;

use super::station::{
    Made, StationDesktop, StationSids, StationSteps, made_fresh, make_station, station_name_of_path,
};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::sids::Sid;
use crate::windows::token::os::{LocalDescriptor, LocalSid, user};
use crate::windows::token::{DESKTOP_MAKE_RIGHTS, STATION_MAKE_RIGHTS, STATION_OPEN_RIGHTS};

/// A failure: the call that failed and its error.
type Failed = (&'static str, io::Error);

/// `ObjectNameInformation`: a window station's object path (`NtQueryObject`).
const OBJECT_NAME: OBJECT_INFORMATION_CLASS = OBJECT_INFORMATION_CLASS(1);

/// The agent's window station and its desktop, closed on drop (the outer runner keeps them for
/// its whole run), the desktop first.
pub(super) struct AgentDesktop {
    _desktop: Desktop,
    _station: Station,
    /// `<station>\Default`, the `lpDesktop` of every process the runners start: the inner
    /// runner and the agent.
    pub(super) path: StationDesktop,
}

impl AgentDesktop {
    /// Makes the agent's window station and desktop `station.rs`'s way: the pool user `pool_name`
    /// (on this computer) logged on with `password` (wiped here as soon as `LogonUserW` has
    /// it), the descriptors naming `sids`. A failure names the call that failed.
    pub(super) fn create(
        pool_name: &str,
        password: &Zeroizing<String>,
        sids: &StationSids<'_>,
    ) -> Result<Self, Failed> {
        let mut steps = OsStation {
            pool_name,
            password,
            pool_user: sids.pool_user,
        };
        let Made {
            station,
            desktop,
            name,
        } = make_station(&mut steps, sids)?;
        let path = StationDesktop::of(&name).ok_or_else(|| {
            (
                "GetUserObjectInformationW",
                io::Error::other("the station's name isn't an unnamed station's"),
            )
        })?;
        Ok(AgentDesktop {
            _desktop: desktop,
            _station: station,
            path,
        })
    }
}

/// A window station handle, closed on drop.
pub(super) struct Station(HWINSTA);

impl Drop for Station {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateWindowStationW or OpenWindowStationW and is closed
        // once, here.
        drop(unsafe { CloseWindowStation(self.0) });
    }
}

/// A desktop handle, closed on drop.
pub(super) struct Desktop(HDESK);

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateDesktopW and is closed once, here.
        drop(unsafe { CloseDesktop(self.0) });
    }
}

/// Windows' steps.
struct OsStation<'a> {
    pool_name: &'a str,
    password: &'a Zeroizing<String>,
    pool_user: &'a Sid,
}

impl StationSteps for OsStation<'_> {
    type Logon = Handle;
    type Station = Station;
    type Desktop = Desktop;
    type Previous = HWINSTA;
    type Error = Failed;

    fn log_on(&mut self) -> Result<Handle, Failed> {
        let user = HSTRING::from(self.pool_name);
        // Sized once, so no reallocation leaves an unwiped copy of the password behind.
        let units = self.password.encode_utf16().count();
        let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(units + 1));
        secret.extend(self.password.encode_utf16());
        secret.push(0);
        let mut token = HANDLE::default();
        // SAFETY: the name, the domain (`.`, this computer) and the password are NUL-terminated
        // and outlive the call; `token` is a live out-pointer, owned by `Handle` once written.
        let logged_on = unsafe {
            LogonUserW(
                &user,
                w!("."),
                PCWSTR(secret.as_ptr()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &raw mut token,
            )
        };
        drop(secret);
        logged_on.map_err(|e| ("LogonUserW", os_error(&e)))?;
        Ok(Handle(token))
    }

    /// Acts as `logon`'s user, then checks the thread's token is the pool user's at
    /// `SecurityImpersonation` or above: Windows gives a mere identification token, with no
    /// error, to a caller it won't let impersonate.
    fn impersonate(&mut self, logon: &Handle) -> Result<(), Failed> {
        let call = "ImpersonateLoggedOnUser";
        // SAFETY: the token is a live logon token with every right (LogonUserW's).
        unsafe { ImpersonateLoggedOnUser(logon.0) }.map_err(|e| (call, os_error(&e)))?;
        let mut thread = HANDLE::default();
        // SAFETY: the current thread's pseudo-handle; opened as the process, so the check of the
        // thread token's own DACL doesn't depend on the impersonation; `thread` is a live
        // out-pointer, owned by `Handle` once written.
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut thread) }
            .map_err(|e| ("OpenThreadToken", os_error(&e)))?;
        let thread = Handle(thread);
        let acting = user(&thread).map_err(|e| ("GetTokenInformation", e))?;
        if acting != *self.pool_user || impersonation_level(&thread)? < SecurityImpersonation.0 {
            return Err((
                call,
                io::Error::other("the thread doesn't act as the pool user"),
            ));
        }
        Ok(())
    }

    /// Unnamed, so Windows names it `Service-0x0-<logon LUID>$`.
    fn create_station(&mut self, sddl: &str) -> Result<Station, Failed> {
        let descriptor = LocalDescriptor::new(sddl).map_err(|e| ("the descriptor", e))?;
        let attributes = attributes(&descriptor)?;
        // SAFETY: no name; the attributes with their descriptor outlive the call; the station is
        // owned by `Station` at once.
        unsafe {
            CreateWindowStationW(
                PCWSTR::null(),
                0,
                STATION_MAKE_RIGHTS,
                Some(&raw const attributes),
            )
        }
        .map(Station)
        .map_err(|e| ("CreateWindowStationW", os_error(&e)))
    }

    fn revert(&mut self) {
        // SAFETY: no arguments; it ends whatever impersonation the calling thread has.
        if unsafe { RevertToSelf() }.is_err() {
            refuse_now("RevertToSelf");
        }
    }

    fn close_logon(&mut self, logon: Handle) {
        drop(logon);
    }

    /// The station's name, from its object path (`NtQueryObject`): `UOI_NAME` needs
    /// `WINSTA_READATTRIBUTES`, which the creation rights (`READ_CONTROL` alone) don't grant.
    fn name(&mut self, station: &Station) -> Result<String, Failed> {
        let call = "NtQueryObject";
        // Eight KiB: an object path is far shorter; `NtQueryObject` fails past the buffer.
        let mut buffer = vec![0u64; 1024];
        let len = u32::try_from(size_of_val(buffer.as_slice()))
            .map_err(|e| (call, io::Error::other(e)))?;
        // SAFETY: the station handle is live; `buffer` is live, writable for `len` bytes and
        // aligned for an OBJECT_NAME_INFORMATION.
        let status = unsafe {
            NtQueryObject(
                Some(HANDLE(station.0.0)),
                OBJECT_NAME,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                len,
                None,
            )
        };
        if status.0 < 0 {
            return Err((
                call,
                io::Error::other(format!("NTSTATUS {:#010x}", status.0.cast_unsigned())),
            ));
        }
        // SAFETY: the call wrote an OBJECT_NAME_INFORMATION at the start of `buffer`; its string
        // points into `buffer` (or is empty).
        let info = unsafe { &*buffer.as_ptr().cast::<OBJECT_NAME_INFORMATION>() };
        let units = usize::from(info.Name.Length) / 2;
        if units == 0 || info.Name.Buffer.is_null() {
            return Err((call, io::Error::other("the station has no object name")));
        }
        // SAFETY: `Length` bytes of UTF-16 at `Buffer`, inside `buffer`, which is still live.
        let wide = unsafe { std::slice::from_raw_parts(info.Name.Buffer.0, units) };
        let path = String::from_utf16(wide).map_err(|e| (call, io::Error::other(e)))?;
        let name = station_name_of_path(&path).map_err(|why| (call, io::Error::other(why)))?;
        if StationDesktop::of(name).is_none() {
            return Err((
                call,
                io::Error::other("the station's name isn't an unnamed station's"),
            ));
        }
        Ok(name.to_owned())
    }

    fn open_station(&mut self, name: &str) -> Result<Station, Failed> {
        // SAFETY: the name is a live HSTRING; the station is owned by `Station` at once.
        unsafe { OpenWindowStationW(&HSTRING::from(name), false, STATION_OPEN_RIGHTS) }
            .map(Station)
            .map_err(|e| ("OpenWindowStationW", os_error(&e)))
    }

    fn set_owner(&mut self, station: &Station, owner: &Sid) -> Result<(), Failed> {
        let call = "SetSecurityInfo (owner)";
        let owner = LocalSid::new(owner).map_err(|e| (call, e))?;
        // SAFETY: the handle is live with WRITE_OWNER; the SID lives in `owner`, which outlives
        // the call.
        let code = unsafe {
            SetSecurityInfo(
                HANDLE(station.0.0),
                SE_WINDOW_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(owner.as_psid()),
                None,
                None,
                None,
            )
        };
        win32(call, code.0)
    }

    fn enter_station(&mut self, station: &Station) -> Result<HWINSTA, Failed> {
        // SAFETY: a query of this process's own window station; the handle isn't closed (it is
        // the process's).
        let previous = unsafe { GetProcessWindowStation() }
            .map_err(|e| ("GetProcessWindowStation", os_error(&e)))?;
        // SAFETY: `station` is a live window station handle.
        unsafe { SetProcessWindowStation(station.0) }
            .map_err(|e| ("SetProcessWindowStation", os_error(&e)))?;
        Ok(previous)
    }

    fn create_desktop(&mut self, sddl: &str) -> Result<Desktop, Failed> {
        let call = "CreateDesktopW";
        let descriptor = LocalDescriptor::new(sddl).map_err(|e| ("the descriptor", e))?;
        let attributes = attributes(&descriptor)?;
        // SAFETY: only this thread's last-error value; cleared so a code left by an earlier
        // call isn't read as this one's.
        unsafe { SetLastError(WIN32_ERROR(0)) };
        // SAFETY: the name is static; `attributes` and its descriptor outlive the call; the
        // desktop is owned by `Desktop` at once.
        let desktop = unsafe {
            CreateDesktopW(
                w!("Default"),
                PCWSTR::null(),
                None,
                DESKTOP_CONTROL_FLAGS::default(),
                DESKTOP_MAKE_RIGHTS,
                Some(&raw const attributes),
            )
        }
        .map(Desktop)
        .map_err(|e| (call, os_error(&e)))?;
        // SAFETY: reads this thread's last-error value, set by the call above.
        let last = unsafe { GetLastError() };
        // A `Default` someone made first: closed by `desktop`'s drop, never used.
        made_fresh(last.0).map_err(|code| {
            (
                call,
                io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX)),
            )
        })?;
        Ok(desktop)
    }

    fn leave_station(&mut self, previous: HWINSTA) {
        // SAFETY: `previous` is this process's own window station, still open.
        if unsafe { SetProcessWindowStation(previous) }.is_err() {
            refuse_now("SetProcessWindowStation (back)");
        }
    }

    fn secure_station(&mut self, station: &Station, sddl: &str) -> Result<(), Failed> {
        secure(HANDLE(station.0.0), sddl)
    }

    fn secure_desktop(&mut self, desktop: &Desktop, sddl: &str) -> Result<(), Failed> {
        secure(HANDLE(desktop.0.0), sddl)
    }
}

/// Ends the process at once with [`RunExit::Token`](super::RunExit::Token), after one stderr line
/// naming `call`: a thread still acting as the pool user, or a process still on the agent's
/// station, must run nothing more (no end path either; the session's marker stays).
#[expect(
    clippy::exit,
    reason = "nothing more may run on this thread or this station, no end path either"
)]
fn refuse_now(call: &str) -> ! {
    let mut stderr = io::stderr().lock();
    drop(
        writeln!(
            stderr,
            "clauseo-sandbox-run: the agent's window station: {call} failed"
        )
        .and_then(|()| stderr.flush()),
    );
    std::process::exit(i32::from_ne_bytes(
        super::RunExit::Token.code().to_ne_bytes(),
    ))
}

/// The impersonation level of the thread token `token`.
fn impersonation_level(token: &Handle) -> Result<i32, Failed> {
    let call = "GetTokenInformation";
    let mut level = SECURITY_IMPERSONATION_LEVEL::default();
    let mut needed = 0u32;
    let len = u32::try_from(size_of::<SECURITY_IMPERSONATION_LEVEL>())
        .map_err(|e| (call, io::Error::other(e)))?;
    // SAFETY: the token is live with TOKEN_QUERY; `level` is the structure the class names,
    // writable for its size; `needed` is a live out-pointer.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenImpersonationLevel,
            Some((&raw mut level).cast::<c_void>()),
            len,
            &raw mut needed,
        )
    }
    .map_err(|e| (call, os_error(&e)))?;
    Ok(level.0)
}

/// Security attributes holding `descriptor`, not inherited.
fn attributes(descriptor: &LocalDescriptor) -> Result<SECURITY_ATTRIBUTES, Failed> {
    Ok(SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
            .map_err(|e| ("the descriptor", io::Error::other(e)))?,
        lpSecurityDescriptor: descriptor.raw(),
        bInheritHandle: false.into(),
    })
}

/// Gives the window object `handle` the protected DACL and the label of `sddl`, as its owner:
/// the creator's entry for the desktop's making goes.
fn secure(handle: HANDLE, sddl: &str) -> Result<(), Failed> {
    let call = "SetSecurityInfo (DACL and label)";
    let descriptor = LocalDescriptor::new(sddl).map_err(|e| ("the descriptor", e))?;
    let dacl = descriptor.dacl().map_err(|e| (call, e))?;
    let sacl = descriptor.sacl().map_err(|e| (call, e))?;
    let what: OBJECT_SECURITY_INFORMATION = DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION
        | LABEL_SECURITY_INFORMATION;
    // SAFETY: the handle is live with WRITE_DAC and WRITE_OWNER (a label needs the latter); the
    // ACLs live in `descriptor`, which outlives the call.
    let code = unsafe {
        SetSecurityInfo(
            handle,
            SE_WINDOW_OBJECT,
            what,
            None,
            None,
            Some(dacl.cast_const()),
            Some(sacl.cast_const()),
        )
    };
    win32(call, code.0)
}

/// A `SetSecurityInfo` result: 0, or its Win32 code as the failure of `call`.
fn win32(call: &'static str, code: u32) -> Result<(), Failed> {
    if code == 0 {
        return Ok(());
    }
    Err((
        call,
        io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX)),
    ))
}
