//! The clipboard and global atom calls the `clipboard` and `atoms` rows make, on both sides (the
//! harness as the desktop user, and the probe inside the sandbox). Declared here against user32 and
//! kernel32 directly: the crate's `windows` features hold neither the clipboard nor `GlobalAlloc`,
//! and an example shouldn't widen the product's feature list.
#![expect(
    unsafe_code,
    reason = "FFI to the clipboard, window and global atom calls"
)]

use std::ffi::c_void;
use std::iter::once;
use std::ptr::{null, null_mut};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;
/// `GMEM_MOVEABLE`.
const GMEM_MOVEABLE: u32 = 0x2;
/// `HWND_MESSAGE`: the parent of a message-only window.
const HWND_MESSAGE: isize = -3;

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> *mut c_void;
    fn SetClipboardData(format: u32, memory: *mut c_void) -> *mut c_void;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *const c_void,
    ) -> *mut c_void;
    fn DestroyWindow(window: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(memory: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(memory: *mut c_void) -> i32;
    fn GlobalFree(memory: *mut c_void) -> *mut c_void;
    fn GlobalAddAtomW(name: *const u16) -> u16;
    fn GlobalFindAtomW(name: *const u16) -> u16;
    fn GlobalDeleteAtom(atom: u16) -> u16;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(once(0)).collect()
}

/// `call` failed: its name and the last error.
fn failed(call: &str) -> String {
    // SAFETY: plain call.
    format!("{call} failed ({})", unsafe { GetLastError() })
}

/// A message-only window, the clipboard's owner while it is written; destroyed on drop.
struct Window(*mut c_void);

impl Window {
    fn new() -> Result<Window, String> {
        let class = wide("STATIC");
        let name = wide("");
        // SAFETY: both strings are NUL-terminated and outlive the call; a message-only window
        // of the system class STATIC needs no instance or parameter.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::without_provenance_mut(HWND_MESSAGE.cast_unsigned()),
                null_mut(),
                null_mut(),
                null(),
            )
        };
        if window.is_null() {
            return Err(failed("CreateWindowExW"));
        }
        Ok(Window(window))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window is this thread's, destroyed once, here.
        unsafe { DestroyWindow(self.0) };
    }
}

/// The clipboard, open; closed on drop.
struct Open;

impl Open {
    fn new(owner: *mut c_void) -> Result<Open, String> {
        // SAFETY: `owner` is null or this thread's live window.
        if unsafe { OpenClipboard(owner) } == 0 {
            return Err(failed("OpenClipboard"));
        }
        Ok(Open)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the clipboard is open in this thread.
        unsafe { CloseClipboard() };
    }
}

/// The clipboard's text.
pub(crate) fn read_text() -> Result<String, String> {
    let _open = Open::new(null_mut())?;
    // SAFETY: the clipboard is open.
    let memory = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if memory.is_null() {
        return Err(failed("GetClipboardData(CF_UNICODETEXT)"));
    }
    // SAFETY: the clipboard's handle is a global memory block while the clipboard is open.
    let locked = unsafe { GlobalLock(memory) }.cast::<u16>();
    if locked.is_null() {
        return Err(failed("GlobalLock"));
    }
    let mut units = Vec::new();
    // SAFETY: CF_UNICODETEXT is NUL-terminated; read up to the NUL (capped at 1 MiB units).
    unsafe {
        while units.len() < 1 << 20 {
            let unit = *locked.add(units.len());
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        GlobalUnlock(memory);
    }
    Ok(String::from_utf16_lossy(&units))
}

/// Replaces the clipboard's contents with `text`.
pub(crate) fn write_text(text: &str) -> Result<(), String> {
    let window = Window::new()?;
    let _open = Open::new(window.0)?;
    // SAFETY: the clipboard is open with an owner window.
    if unsafe { EmptyClipboard() } == 0 {
        return Err(failed("EmptyClipboard"));
    }
    let units = wide(text);
    let bytes = units.len() * 2;
    // SAFETY: plain allocation; freed below unless the clipboard takes it.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if memory.is_null() {
        return Err(failed("GlobalAlloc"));
    }
    // SAFETY: `memory` is a live block of `bytes` bytes; `units` fills it exactly.
    unsafe {
        let locked = GlobalLock(memory).cast::<u16>();
        if locked.is_null() {
            GlobalFree(memory);
            return Err(failed("GlobalLock"));
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), locked, units.len());
        GlobalUnlock(memory);
    }
    // SAFETY: the clipboard is open and emptied by this owner; on success it owns `memory`.
    if unsafe { SetClipboardData(CF_UNICODETEXT, memory) }.is_null() {
        let why = failed("SetClipboardData(CF_UNICODETEXT)");
        // SAFETY: the clipboard didn't take the block; freed once.
        unsafe { GlobalFree(memory) };
        return Err(why);
    }
    Ok(())
}

/// Adds the global atom `name`.
pub(crate) fn add_atom(name: &str) -> Result<u16, String> {
    let name = wide(name);
    // SAFETY: the name is NUL-terminated and outlives the call.
    match unsafe { GlobalAddAtomW(name.as_ptr()) } {
        0 => Err(failed("GlobalAddAtomW")),
        atom => Ok(atom),
    }
}

/// The global atom `name`, 0 when none is found.
pub(crate) fn find_atom(name: &str) -> u16 {
    let name = wide(name);
    // SAFETY: the name is NUL-terminated and outlives the call.
    unsafe { GlobalFindAtomW(name.as_ptr()) }
}

/// Deletes one reference to the global atom `atom`; whether it worked.
pub(crate) fn delete_atom(atom: u16) -> bool {
    // SAFETY: plain call; a wrong atom only fails.
    unsafe { GlobalDeleteAtom(atom) == 0 }
}
