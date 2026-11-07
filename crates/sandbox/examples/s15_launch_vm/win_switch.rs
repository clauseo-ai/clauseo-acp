//! `SwitchDesktop`, the UI limits' third effect (`probe job-ui` and `probe ui-control`): a new
//! desktop on the caller's own window station (the small heap `win_desk.rs` asks for), a switch to
//! it, a switch straight back to the original (the input desktop, or this thread's own), and the
//! new desktop closed. The job's `DESKTOP` UI limit refuses both the create and the switch with 5.
//! On a window station that isn't interactive the switch fails whatever the job, so on CI (session
//! 0) the effect is information only.
#![expect(unsafe_code, reason = "FFI to the desktop calls")]

use std::io;

use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP,
    GetThreadDesktop, HDESK, OpenInputDesktop, SwitchDesktop,
};
use windows::Win32::System::Threading::GetCurrentThreadId;

use super::win::code_of;
use super::win_desk::SMALL_HEAP_KB;

#[link(name = "user32")]
unsafe extern "system" {
    /// As `win_desk.rs` declares it: the crate's binding takes a `DEVMODEW` this never passes.
    fn CreateDesktopExW(
        name: *const u16,
        device: *const u16,
        mode: *const std::ffi::c_void,
        flags: u32,
        access: u32,
        attributes: *const std::ffi::c_void,
        heap_kb: u32,
        reserved: *const std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
}

/// What [`switch_desktop`] got.
#[derive(Debug)]
pub(crate) struct Switched {
    /// `SwitchDesktop`'s code (0 when it switched), or `create-failed <code>` when the desktop
    /// to switch to couldn't be made: the line's value.
    pub(crate) code: String,
    /// The switch back's code, when it switched.
    pub(crate) back: Option<u32>,
}

/// Makes a desktop, switches to it, switches back at once and closes it.
pub(crate) fn switch_desktop() -> Switched {
    let name: Vec<u16> = format!("ClauseoS15Sw-{}", std::process::id())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the name is NUL-terminated and outlives the call; no device, mode or attributes.
    let made = unsafe {
        CreateDesktopExW(
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DESKTOP_READOBJECTS.0 | DESKTOP_SWITCHDESKTOP.0,
            std::ptr::null(),
            SMALL_HEAP_KB,
            std::ptr::null(),
        )
    };
    if made.is_null() {
        let code = io::Error::last_os_error().raw_os_error().unwrap_or(-1);
        return Switched {
            code: format!("create-failed {code}"),
            back: None,
        };
    }
    let made = HDESK(made);
    // SAFETY: plain call; a desktop that opens is closed below.
    let input =
        unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) }.ok();
    // SAFETY: `made` is live with DESKTOP_SWITCHDESKTOP.
    let switched = match unsafe { SwitchDesktop(made) } {
        Ok(()) => Switched {
            code: "0".to_owned(),
            back: Some(switch_back(input)),
        },
        Err(error) => Switched {
            code: code_of(&error).to_string(),
            back: None,
        },
    };
    if let Some(input) = input {
        // SAFETY: opened above, closed once.
        drop(unsafe { CloseDesktop(input) });
    }
    // SAFETY: made above, closed once (no longer the input desktop when the switch back worked).
    drop(unsafe { CloseDesktop(made) });
    switched
}

/// Switches to `input` (the input desktop before the switch), or to this thread's own desktop
/// when that wasn't open or the switch fails; the last call's code, 0 when it switched.
fn switch_back(input: Option<HDESK>) -> u32 {
    if let Some(input) = input {
        // SAFETY: `input` is live with DESKTOP_SWITCHDESKTOP.
        if unsafe { SwitchDesktop(input) }.is_ok() {
            return 0;
        }
    }
    // SAFETY: plain call; the thread's desktop handle isn't ours to close.
    match unsafe { GetThreadDesktop(GetCurrentThreadId()) } {
        // SAFETY: the thread's desktop handle is live.
        Ok(own) => match unsafe { SwitchDesktop(own) } {
            Ok(()) => 0,
            Err(error) => code_of(&error),
        },
        Err(error) => code_of(&error),
    }
}
