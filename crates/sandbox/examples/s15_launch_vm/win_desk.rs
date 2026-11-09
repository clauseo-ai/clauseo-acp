//! The Win32 calls of the `desktop` row: the harness's dump of `WinSta0`'s and
//! `WinSta0\Default`'s DACLs as SDDL, and the probe's tries (its own station's name, opening
//! `WinSta0` and then `Default` on it, `SwitchDesktop` on its own desktop, a GDI capture of the
//! screen it can reach), `CreateDesktopW` on the caller's own station (`job-ui`'s effect). The
//! harness's control station is `win_station.rs`'s. Each gives a plain value or the Win32 code,
//! never a panic.
#![expect(
    unsafe_code,
    reason = "FFI to the window station, desktop, security descriptor and GDI calls"
)]

use std::io;

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetBitmapBits,
    GetDC, GetDeviceCaps, HGDIOBJ, HORZRES, ReleaseDC, SRCCOPY, SelectObject, VERTRES,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_WINDOW_OBJECT,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::READ_CONTROL;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, CloseWindowStation, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
    DESKTOP_SWITCHDESKTOP, GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW,
    HDESK, OpenDesktopW, OpenWindowStationW, SetProcessWindowStation, SwitchDesktop, UOI_NAME,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::core::{HSTRING, PWSTR};

use super::win::code_of;

/// `WINSTA_ENUMDESKTOPS | WINSTA_READATTRIBUTES | WINSTA_ACCESSCLIPBOARD`.
const WINSTA_ASK: u32 = 0x0001 | 0x0002 | 0x0004;
/// The most of the screen the capture copies, each way: enough to tell pixels from none.
const CAPTURE_CAP: i32 = 256;

/// Which object's DACL to dump.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UserObject {
    /// The window station `WinSta0`.
    WinSta0,
    /// The desktop `Default` on this process's window station.
    Default,
}

/// The DACL of `object` as SDDL, read with `READ_CONTROL`; why when it can't be.
pub(crate) fn dacl_sddl(object: UserObject) -> Result<String, String> {
    let handle = match object {
        UserObject::WinSta0 => {
            // SAFETY: the name is a live HSTRING; the handle is closed below.
            let station =
                unsafe { OpenWindowStationW(&HSTRING::from("WinSta0"), false, READ_CONTROL.0) }
                    .map_err(|e| format!("OpenWindowStationW(WinSta0) failed ({})", code_of(&e)))?;
            let sddl = sddl_of(HANDLE(station.0));
            // SAFETY: opened above, closed once.
            drop(unsafe { CloseWindowStation(station) });
            return sddl;
        }
        UserObject::Default => {
            // SAFETY: as above.
            unsafe {
                OpenDesktopW(
                    &HSTRING::from("Default"),
                    DESKTOP_CONTROL_FLAGS::default(),
                    false,
                    READ_CONTROL.0,
                )
            }
            .map_err(|e| format!("OpenDesktopW(Default) failed ({})", code_of(&e)))?
        }
    };
    let sddl = sddl_of(HANDLE(handle.0));
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseDesktop(handle) });
    sddl
}

/// `GetSecurityInfo(DACL)` of the user object `handle`, as SDDL.
fn sddl_of(handle: HANDLE) -> Result<String, String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is live with READ_CONTROL; the descriptor is freed below.
    let code = unsafe {
        GetSecurityInfo(
            handle,
            SE_WINDOW_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if code.0 != 0 {
        return Err(format!("GetSecurityInfo failed ({})", code.0));
    }
    let mut text = PWSTR::null();
    // SAFETY: `descriptor` is the live one GetSecurityInfo gave; `text` is a live out-pointer.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    };
    let result = match converted {
        // SAFETY: the call wrote a NUL-terminated string.
        Ok(()) => unsafe { text.to_string() }.map_err(|e| e.to_string()),
        Err(error) => Err(format!(
            "ConvertSecurityDescriptorToStringSecurityDescriptorW failed ({})",
            code_of(&error)
        )),
    };
    if !text.is_null() {
        // SAFETY: allocated by the conversion with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    }
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    result
}

/// The name of this thread's desktop; the code when it can't be read.
pub(crate) fn own_desktop_name() -> Result<String, u32> {
    // SAFETY: plain call; the thread's desktop handle isn't closed (it isn't ours to close).
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.map_err(|e| code_of(&e))?;
    object_name(HANDLE(desktop.0))
}

/// The name of this process's window station; the code when it can't be read.
pub(crate) fn own_station_name() -> Result<String, u32> {
    // SAFETY: plain call; the process's station handle isn't closed (it isn't ours to close).
    let station = unsafe { GetProcessWindowStation() }.map_err(|e| code_of(&e))?;
    object_name(HANDLE(station.0))
}

/// `GetUserObjectInformationW(UOI_NAME)` of the user object `handle`.
fn object_name(handle: HANDLE) -> Result<String, u32> {
    let mut name = vec![0u16; 256];
    let len = u32::try_from(size_of_val(name.as_slice())).unwrap_or(0);
    // SAFETY: the handle is live; `name` is writable for `len` bytes.
    unsafe {
        GetUserObjectInformationW(handle, UOI_NAME, Some(name.as_mut_ptr().cast()), len, None)
    }
    .map_err(|e| code_of(&e))?;
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    Ok(String::from_utf16_lossy(
        name.get(..end).unwrap_or_default(),
    ))
}

/// What the agent got trying the interactive window station's `Default` desktop, the
/// screen-capture path: each a Win32 code, 0 when it succeeded; a later step is `None` when an
/// earlier one failed.
#[derive(Debug)]
pub(crate) struct WinSta0Path {
    /// `OpenWindowStationW("WinSta0", WINSTA_ENUMDESKTOPS | WINSTA_READATTRIBUTES |
    /// WINSTA_ACCESSCLIPBOARD)`.
    pub(crate) open: u32,
    /// `SetProcessWindowStation` to it.
    pub(crate) set: Option<u32>,
    /// `OpenDesktopW("Default", DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP)` there.
    pub(crate) desktop: Option<u32>,
}

/// Tries `WinSta0`, and when it opens, `Default` on it; the process's own station is put
/// back and every handle closed before this returns.
pub(crate) fn try_winsta0() -> WinSta0Path {
    let mut got = WinSta0Path {
        open: 0,
        set: None,
        desktop: None,
    };
    // SAFETY: the name is a live HSTRING; a station that opens is closed below.
    let station = match unsafe { OpenWindowStationW(&HSTRING::from("WinSta0"), false, WINSTA_ASK) }
    {
        Ok(station) => station,
        Err(error) => {
            got.open = code_of(&error);
            return got;
        }
    };
    // SAFETY: plain call; the process's own station isn't ours to close.
    let own = unsafe { GetProcessWindowStation() };
    // SAFETY: the station is live.
    match unsafe { SetProcessWindowStation(station) } {
        Ok(()) => {
            got.set = Some(0);
            let access = DESKTOP_READOBJECTS.0 | DESKTOP_SWITCHDESKTOP.0;
            // SAFETY: the name is a live HSTRING; a desktop that opens is closed at once.
            let opened = unsafe {
                OpenDesktopW(
                    &HSTRING::from("Default"),
                    DESKTOP_CONTROL_FLAGS::default(),
                    false,
                    access,
                )
            };
            got.desktop = Some(match opened {
                Ok(desktop) => {
                    // SAFETY: opened above, closed once.
                    drop(unsafe { CloseDesktop(desktop) });
                    0
                }
                Err(error) => code_of(&error),
            });
            if let Ok(own) = own {
                // SAFETY: the process's own station handle is live.
                drop(unsafe { SetProcessWindowStation(own) });
            }
        }
        Err(error) => got.set = Some(code_of(&error)),
    }
    // SAFETY: opened above and no longer the process's station; closed once.
    drop(unsafe { CloseWindowStation(station) });
    got
}

#[link(name = "user32")]
unsafe extern "system" {
    /// Declared here: the crate's binding takes a `DEVMODEW` this call never passes.
    fn CreateDesktopW(
        name: *const u16,
        device: *const u16,
        mode: *const std::ffi::c_void,
        flags: u32,
        access: u32,
        attributes: *const std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    /// As `CreateDesktopW`, with the desktop's heap size in KB.
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

/// The heap the probes' desktops ask for, in KB. Every control variant, the lawyer's plain token
/// included, got 8 (`ERROR_NOT_ENOUGH_MEMORY`) from `CreateDesktopW` with the default heap on a
/// fresh station in CI's session 0 (harness run 42367294132): a second desktop of the default
/// size doesn't fit there. A small heap asks for little, so a refusal is about access, not room.
pub(crate) const SMALL_HEAP_KB: u32 = 128;

/// `CreateDesktopW`'s code with the default heap, printed as information beside the
/// small-heap code [`create_desktop_code`] gives.
pub(crate) fn create_desktop_default_heap_code() -> u32 {
    create_desktop_sized(std::ptr::null(), None)
}

/// The code `CreateDesktopW` gives for a new desktop on this process's own window station; 0
/// when it was made (it is closed at once). The job's `DESKTOP` UI limit refuses it with 5.
pub(crate) fn create_desktop_code() -> u32 {
    create_desktop_with(std::ptr::null())
}

/// As [`create_desktop_code`], with explicit `SECURITY_ATTRIBUTES` whose descriptor is `sddl`
/// (the control's variant (c)); `Err` with the code when `sddl` doesn't convert.
pub(crate) fn create_desktop_code_sa(sddl: &str) -> Result<u32, u32> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the text is a live HSTRING; `descriptor` is a live out-pointer, freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(|e| code_of(&e))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let code = create_desktop_with((&raw const attributes).cast());
    // SAFETY: allocated by the conversion with LocalAlloc, freed once, after its last use.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    Ok(code)
}

/// [`create_desktop_code`]'s call, with `attributes` (null, or a live `SECURITY_ATTRIBUTES`),
/// on a [`SMALL_HEAP_KB`] heap.
fn create_desktop_with(attributes: *const std::ffi::c_void) -> u32 {
    create_desktop_sized(attributes, Some(SMALL_HEAP_KB))
}

/// `CreateDesktopExW` with `heap_kb`, or `CreateDesktopW` (the default heap) with `None`.
fn create_desktop_sized(attributes: *const std::ffi::c_void, heap_kb: Option<u32>) -> u32 {
    let name: Vec<u16> = format!("ClauseoS15Fx-{}", std::process::id())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the name is NUL-terminated and outlives the call; no device or mode; `attributes`
    // is null or live for the call (the caller's promise).
    let desktop = unsafe {
        match heap_kb {
            Some(heap_kb) => CreateDesktopExW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                DESKTOP_READOBJECTS.0,
                attributes,
                heap_kb,
                std::ptr::null(),
            ),
            None => CreateDesktopW(
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                DESKTOP_READOBJECTS.0,
                attributes,
            ),
        }
    };
    if desktop.is_null() {
        return u32::try_from(last()).unwrap_or(u32::MAX);
    }
    // SAFETY: made above, closed once.
    drop(unsafe { CloseDesktop(HDESK(desktop)) });
    0
}

/// The code `SwitchDesktop` on this thread's own desktop gives; 0 when it switched.
pub(crate) fn switch_own_code() -> u32 {
    // SAFETY: plain call; the thread's desktop handle isn't closed.
    let desktop = match unsafe { GetThreadDesktop(GetCurrentThreadId()) } {
        Ok(desktop) => desktop,
        Err(error) => return code_of(&error),
    };
    // SAFETY: the desktop handle is live.
    match unsafe { SwitchDesktop(desktop) } {
        Ok(()) => 0,
        Err(error) => code_of(&error),
    }
}

/// What a GDI capture of the screen DC (`GetDC(NULL)`, then `BitBlt` into a memory DC) got.
#[derive(Debug)]
pub(crate) struct Capture {
    /// The size copied.
    pub(crate) width: i32,
    /// The size copied.
    pub(crate) height: i32,
    /// `BitBlt`'s code, 0 when it copied.
    pub(crate) blit: u32,
    /// The bitmap's bytes read back.
    pub(crate) bytes: usize,
    /// How many of them are non-zero.
    pub(crate) nonzero: usize,
}

/// A GDI capture of up to [`CAPTURE_CAP`] square of the screen this thread's desktop shows; why
/// when a call before the copy failed.
pub(crate) fn capture() -> Result<Capture, String> {
    // SAFETY: no window: the screen's DC, released below.
    let screen = unsafe { GetDC(None) };
    if screen.is_invalid() {
        return Err(format!("GetDC(NULL) failed ({})", last()));
    }
    let result = capture_from(screen);
    // SAFETY: the DC came from GetDC(NULL) and is released once.
    unsafe { ReleaseDC(None, screen) };
    result
}

fn capture_from(screen: windows::Win32::Graphics::Gdi::HDC) -> Result<Capture, String> {
    // SAFETY: the screen DC is live.
    let (wide, high) = unsafe {
        (
            GetDeviceCaps(Some(screen), HORZRES),
            GetDeviceCaps(Some(screen), VERTRES),
        )
    };
    let (width, height) = (wide.clamp(1, CAPTURE_CAP), high.clamp(1, CAPTURE_CAP));
    // SAFETY: the screen DC is live; the memory DC is deleted below.
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    if memory.is_invalid() {
        return Err(format!("CreateCompatibleDC failed ({})", last()));
    }
    // SAFETY: as above; the bitmap is deleted below.
    let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
    if bitmap.is_invalid() {
        // SAFETY: made above, deleted once.
        drop(unsafe { DeleteDC(memory) }.ok());
        return Err(format!("CreateCompatibleBitmap failed ({})", last()));
    }
    // SAFETY: both are live; the old object is put back before either is deleted.
    let old = unsafe { SelectObject(memory, HGDIOBJ(bitmap.0)) };
    // SAFETY: both DCs are live and the bitmap is selected into the memory DC.
    let blit = match unsafe { BitBlt(memory, 0, 0, width, height, Some(screen), 0, 0, SRCCOPY) } {
        Ok(()) => 0,
        Err(error) => code_of(&error),
    };
    // SAFETY: puts back the memory DC's own bitmap.
    unsafe { SelectObject(memory, old) };
    let size = usize::try_from(width)
        .unwrap_or(0)
        .saturating_mul(usize::try_from(height).unwrap_or(0))
        .saturating_mul(8);
    let mut bits = vec![0u8; size];
    // SAFETY: the bitmap is live and no longer selected; `bits` is writable for its length.
    let read = unsafe {
        GetBitmapBits(
            bitmap,
            i32::try_from(bits.len()).unwrap_or(i32::MAX),
            bits.as_mut_ptr().cast(),
        )
    };
    let bytes = usize::try_from(read).unwrap_or(0);
    let nonzero = bits
        .get(..bytes)
        .unwrap_or_default()
        .iter()
        .filter(|b| **b != 0)
        .count();
    // SAFETY: made above, deleted once each.
    unsafe {
        drop(DeleteObject(HGDIOBJ(bitmap.0)).ok());
        drop(DeleteDC(memory).ok());
    }
    Ok(Capture {
        width,
        height,
        blit,
        bytes,
        nonzero,
    })
}

/// The thread's last Win32 error code.
fn last() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(-1)
}

/// The ACEs of the SDDL `sddl`'s DACL, each `(…)` with its parentheses.
pub(crate) fn aces(sddl: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = sddl;
    while let Some(start) = rest.find('(') {
        let Some(after) = rest.get(start..) else {
            break;
        };
        let Some(end) = after.find(')') else {
            break;
        };
        found.push(after.get(..=end).unwrap_or_default().to_owned());
        rest = after.get(end + 1..).unwrap_or_default();
    }
    found
}

/// The trustee SID (or alias) of the ACE `ace`, `(type;flags;rights;object;inherited;sid)`.
pub(crate) fn ace_sid(ace: &str) -> &str {
    ace.trim_matches(['(', ')'])
        .split(';')
        .nth(5)
        .unwrap_or_default()
}
