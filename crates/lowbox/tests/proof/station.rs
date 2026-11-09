//! The private window station and its desktop `Default`, made by the owner with the recipe's
//! final descriptors (recipe step 3): SYSTEM, the owner (`READ_CONTROL | WRITE_DAC |
//! WRITE_OWNER` only), the test account and the packages (A for chrome, and B for the probe,
//! whose user32 must attach to the station and desktop to start at all), label Low. They are made
//! with a creation descriptor that lets the owner make the desktop, then given the final one.
#![expect(
    unsafe_code,
    reason = "FFI to the window station, desktop and SetSecurityInfo calls"
)]

use std::ffi::c_void;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::Authorization::{SE_WINDOW_OBJECT, SetSecurityInfo};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, CreateWindowStationW, GetProcessWindowStation, HDESK,
    HWINSTA, OpenWindowStationW, SetProcessWindowStation,
};
use windows::core::{PCWSTR, w};

use super::win::{Res, Sd, failed, failed_last, wide};

windows::core::link!("user32.dll" "system" fn CreateDesktopW(
    desktop: PCWSTR,
    device: PCWSTR,
    devmode: *const c_void,
    flags: u32,
    access: u32,
    attributes: *const SECURITY_ATTRIBUTES
) -> HDESK);

/// `CWF_CREATE_ONLY`: fail when a station of the name exists.
const CREATE_ONLY: u32 = 1;
/// `READ_CONTROL | WRITE_DAC | WRITE_OWNER | WINSTA_CREATEDESKTOP`, as the recipe held it.
const STATION_ACCESS: u32 = 0x000E_0008;
/// `READ_CONTROL | WRITE_DAC | WRITE_OWNER` and the desktop rights the recipe held.
const DESKTOP_ACCESS: u32 = 0x000E_00C7;
/// `READ_CONTROL`: enough to see the station exists.
const READ_CONTROL: u32 = 0x0002_0000;

/// The station and desktop, closed by [`Station::close`] or on drop.
pub(crate) struct Station {
    /// The station.
    winsta: HWINSTA,
    /// Its desktop `Default`.
    desktop: HDESK,
    /// The station's name.
    name: String,
}

impl Station {
    /// Makes the station `name` and its desktop `Default` for the owner `owner`, the test
    /// account `user` and the `packages` (SID texts).
    pub(crate) fn create(name: &str, owner: &str, user: &str, packages: &[&str]) -> Res<Station> {
        let creation = Sd::from_sddl(&format!(
            "D:P(A;;0xF037F;;;SY)(A;;{STATION_ACCESS:#x};;;{owner})"
        ))?;
        let attributes = creation.attributes();
        let text = wide(name);
        // SAFETY: a NUL-terminated name and attributes whose descriptor outlives the call.
        let station = unsafe {
            CreateWindowStationW(
                PCWSTR(text.as_ptr()),
                CREATE_ONLY,
                STATION_ACCESS,
                Some(&raw const attributes),
            )
        }
        .map_err(|e| failed("CreateWindowStationW", &e))?;
        let mut made = Station {
            winsta: station,
            desktop: HDESK::default(),
            name: name.to_owned(),
        };
        made.desktop = desktop_on(station, owner)?;
        let ace = |mask: &str| -> String {
            packages
                .iter()
                .map(|package| format!("(A;;{mask};;;{package})"))
                .collect::<Vec<String>>()
                .concat()
        };
        let (station_aces, desktop_aces) = (ace("0x2007F"), ace("0x200C7"));
        secure(
            HANDLE(made.winsta.0),
            &format!(
                "D:P(A;;0xF037F;;;SY)(A;;0xE0000;;;{owner})(A;;0x2007F;;;{user})\
                 {station_aces}S:(ML;;NW;;;LW)"
            ),
            "station",
        )?;
        secure(
            HANDLE(made.desktop.0),
            &format!(
                "D:P(A;;0xF01FF;;;SY)(A;;0xE0000;;;{owner})(A;;0x200C7;;;{user})\
                 {desktop_aces}S:(ML;;NW;;;LW)"
            ),
            "desktop",
        )?;
        Ok(made)
    }

    /// The station's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// `<station>\Default`, for `STARTUPINFOW::lpDesktop`.
    pub(crate) fn desktop_path(&self) -> String {
        format!(r"{}\Default", self.name)
    }

    /// Whether a station of this name opens by name now.
    fn exists(&self) -> bool {
        let text = wide(&self.name);
        // SAFETY: a NUL-terminated name; the handle is closed below.
        match unsafe { OpenWindowStationW(PCWSTR(text.as_ptr()), false, READ_CONTROL) } {
            Ok(opened) => {
                // SAFETY: the station was just opened and is closed once, here.
                let _leak_on_failure = unsafe { CloseWindowStation(opened) }.is_ok();
                true
            }
            Err(_) => false,
        }
    }

    /// Closes the desktop, then the station, then checks the station no longer opens by name
    /// (with no process left on it, the last close deletes it).
    pub(crate) fn close(&mut self) -> Res<String> {
        if !self.desktop.is_invalid() {
            // SAFETY: the desktop was made by `create` and is closed once (then forgotten).
            let closed = unsafe { CloseDesktop(self.desktop) };
            self.desktop = HDESK::default();
            closed.map_err(|e| failed("CloseDesktop", &e))?;
        }
        if !self.winsta.is_invalid() {
            // SAFETY: as above, for the station.
            let closed = unsafe { CloseWindowStation(self.winsta) };
            self.winsta = HWINSTA::default();
            closed.map_err(|e| failed("CloseWindowStation", &e))?;
        }
        if self.exists() {
            return Err("closed both, but the station still opens by name".to_owned());
        }
        Ok("closed the desktop and the station; OpenWindowStationW by name now fails".to_owned())
    }
}

impl Drop for Station {
    fn drop(&mut self) {
        let _closed_or_not: Res<String> = self.close();
    }
}

/// Makes the desktop `Default` on `station`, switching this process to the station for the call
/// only.
fn desktop_on(station: HWINSTA, owner: &str) -> Res<HDESK> {
    let creation = Sd::from_sddl(&format!(
        "D:P(A;;0xF01FF;;;SY)(A;;{DESKTOP_ACCESS:#x};;;{owner})"
    ))?;
    let attributes = creation.attributes();
    // SAFETY: reads this process's station handle (not owned: never closed).
    let own =
        unsafe { GetProcessWindowStation() }.map_err(|e| failed("GetProcessWindowStation", &e))?;
    // SAFETY: `station` is open; the process is switched back below.
    unsafe { SetProcessWindowStation(station) }
        .map_err(|e| failed("SetProcessWindowStation", &e))?;
    // SAFETY: a constant name, no device or mode, attributes whose descriptor outlives the call.
    let desktop = unsafe {
        CreateDesktopW(
            w!("Default"),
            PCWSTR::null(),
            std::ptr::null(),
            0,
            DESKTOP_ACCESS,
            &raw const attributes,
        )
    };
    let made = if desktop.is_invalid() {
        Err(failed_last("CreateDesktopW"))
    } else {
        Ok(desktop)
    };
    // SAFETY: `own` is this process's station from above.
    unsafe { SetProcessWindowStation(own) }
        .map_err(|e| failed("SetProcessWindowStation (back)", &e))?;
    made
}

/// Gives the station or desktop `handle` the protected DACL and the label of `sddl`.
fn secure(handle: HANDLE, sddl: &str, what: &str) -> Res<()> {
    let sd = Sd::from_sddl(sddl)?;
    let (dacl, sacl) = (sd.dacl()?, sd.sacl()?);
    // SAFETY: an open handle with WRITE_DAC and WRITE_OWNER; the ACLs live in `sd`.
    let rc = unsafe {
        SetSecurityInfo(
            handle,
            SE_WINDOW_OBJECT,
            DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION
                | LABEL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            Some(sacl),
        )
    };
    if rc.0 != 0 {
        return Err(format!("SetSecurityInfo({what}) failed with {}", rc.0));
    }
    Ok(())
}
