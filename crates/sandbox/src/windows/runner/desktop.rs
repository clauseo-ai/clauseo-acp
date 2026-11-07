//! The agent's window station and desktop (Windows), made by the outer runner (the desktop
//! user) before the inner runner joins the job, so the job can carry the desktop UI limit.
//!
//! A window station of its own, `ClauseoRun-<16 hex>`, holding one desktop, `Default`, each with
//! a descriptor naming only `SYSTEM` and the pool user (and the matter SID when the launch
//! restricts with one), labelled Low (`token::station_sddl`, `token::desktop_sddl`). The (B)
//! token passes both checks on them through the pool user's SID, a restricting SID too. Neither
//! the lawyer's `WinSta0` (whose DACL names the logon SID, deny-only in the token) nor the
//! service window station a runner without a desktop session gets grants the token anything:
//! user32 then fails to start the agent (`0xC0000142`, `STATUS_DLL_INIT_FAILED`; the first
//! Windows runs of the harness). A station of its own also keeps the agent's clipboard and
//! global atoms apart from the lawyer's, whatever the job's UI limits.
#![expect(unsafe_code, reason = "FFI to the window station and desktop calls")]

use std::io;

use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW, DESKTOP_CONTROL_FLAGS,
    GetProcessWindowStation, HDESK, HWINSTA, SetProcessWindowStation,
};
use windows::core::{HSTRING, PCWSTR};

use crate::windows::acl::object::os_error;
use crate::windows::token::os::LocalDescriptor;

/// `GENERIC_ALL`, the access the outer runner asks for on what it makes.
const GENERIC_ALL: u32 = 0x1000_0000;

/// The agent's window station and its desktop, closed on drop (the outer runner keeps them for
/// its whole run).
pub(super) struct AgentDesktop {
    station: HWINSTA,
    desktop: HDESK,
    /// `<station>\Default`, for the agent's `lpDesktop`.
    pub(super) path: String,
}

impl AgentDesktop {
    /// Makes the window station `name` with the descriptor `station_sddl` and its desktop
    /// `Default` with `desktop_sddl`. This process's window station is switched to the new one
    /// only while the desktop is made (a desktop is made on the caller's station), then back.
    pub(super) fn create(name: &str, station_sddl: &str, desktop_sddl: &str) -> io::Result<Self> {
        let station_descriptor = LocalDescriptor::new(station_sddl)?;
        let desktop_descriptor = LocalDescriptor::new(desktop_sddl)?;
        let attributes = |descriptor: &LocalDescriptor| -> io::Result<SECURITY_ATTRIBUTES> {
            Ok(SECURITY_ATTRIBUTES {
                nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                    .map_err(io::Error::other)?,
                lpSecurityDescriptor: descriptor.raw(),
                bInheritHandle: false.into(),
            })
        };
        let station_attributes = attributes(&station_descriptor)?;
        let desktop_attributes = attributes(&desktop_descriptor)?;
        // SAFETY: the name is a live HSTRING and the attributes with their descriptor outlive
        // the call; the station is closed by `Drop` (or below on failure).
        let station = unsafe {
            CreateWindowStationW(
                &HSTRING::from(name),
                0,
                GENERIC_ALL,
                Some(&raw const station_attributes),
            )
        }
        .map_err(|e| os_error(&e))?;
        let made = make_desktop(station, &desktop_attributes);
        match made {
            Ok(desktop) => Ok(AgentDesktop {
                station,
                desktop,
                path: format!(r"{name}Default"),
            }),
            Err(error) => {
                // SAFETY: the station came from CreateWindowStationW and is closed once, here.
                drop(unsafe { CloseWindowStation(station) });
                Err(error)
            }
        }
    }
}

/// Makes the desktop `Default` on `station`, switching this process to it for the call only.
fn make_desktop(station: HWINSTA, attributes: &SECURITY_ATTRIBUTES) -> io::Result<HDESK> {
    // SAFETY: a query of this process's own window station; the handle isn't closed (it is the
    // process's).
    let old = unsafe { GetProcessWindowStation() }.map_err(|e| os_error(&e))?;
    // SAFETY: `station` is a live window station this process made with every right.
    unsafe { SetProcessWindowStation(station) }.map_err(|e| os_error(&e))?;
    // SAFETY: the name is a live HSTRING; `attributes` and its descriptor outlive the call; the
    // desktop is closed by `AgentDesktop`'s drop.
    let desktop = unsafe {
        CreateDesktopW(
            &HSTRING::from("Default"),
            PCWSTR::null(),
            None,
            DESKTOP_CONTROL_FLAGS::default(),
            GENERIC_ALL,
            Some(attributes),
        )
    }
    .map_err(|e| os_error(&e));
    // SAFETY: `old` is this process's own window station, still open; a failure here leaves the
    // process on the new station, which only this runner's own (window-less) work uses.
    drop(unsafe { SetProcessWindowStation(old) });
    desktop
}

impl Drop for AgentDesktop {
    fn drop(&mut self) {
        // SAFETY: both came from the create calls above and are closed once, here, the desktop
        // first.
        unsafe {
            drop(CloseDesktop(self.desktop));
            drop(CloseWindowStation(self.station));
        }
    }
}
