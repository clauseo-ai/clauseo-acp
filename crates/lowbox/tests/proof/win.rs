//! The row's small Win32 helpers, shared by the owner's and the helper's modules: wide strings,
//! error codes, randomness, descriptors from SDDL and back, SIDs as text, `Sleep` and a tick clock.
#![expect(
    unsafe_code,
    reason = "FFI to the SDDL, SID, token, random-number, Sleep and tick-count calls"
)]

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl, GetTokenInformation,
    OBJECT_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken, Sleep};
use windows::core::{BOOL, PCWSTR, PWSTR};

windows::core::link!("bcryptprimitives.dll" "system" fn ProcessPrng(
    data: *mut u8,
    len: usize
) -> BOOL);

/// The row's results: a failure is a sentence naming the call and its code.
pub(crate) type Res<T> = Result<T, String>;

/// `text` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

/// The thread's last Win32 error.
pub(crate) fn last_error() -> u32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .map_or(0, i32::cast_unsigned)
}

/// The Win32 code inside a `windows` error (its HRESULT when it is not one).
pub(crate) fn code(error: &windows::core::Error) -> u32 {
    let hresult = error.code().0.cast_unsigned();
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    }
}

/// The failure sentence for `call`.
pub(crate) fn failed(call: &str, error: &windows::core::Error) -> String {
    let code = code(error);
    format!("{call} failed with error {code} ({code:#x})")
}

/// The failure sentence for `call` from the thread's last error.
pub(crate) fn failed_last(call: &str) -> String {
    let code = last_error();
    format!("{call} failed with error {code} ({code:#x})")
}

/// `len` random bytes from the system's generator.
pub(crate) fn random_bytes(len: usize) -> Vec<u8> {
    let mut out = vec![0_u8; len];
    // SAFETY: `out` is writable for `len` bytes and outlives the call.
    let ok = unsafe { ProcessPrng(out.as_mut_ptr(), len) };
    assert!(ok.as_bool(), "ProcessPrng failed");
    out
}

/// `chars` random lowercase hex digits.
pub(crate) fn random_hex(chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text: String = random_bytes(chars.div_ceil(2))
        .iter()
        .flat_map(|b| [HEX[usize::from(b >> 4)], HEX[usize::from(b & 15)]])
        .map(char::from)
        .collect();
    text.truncate(chars);
    text
}

/// A random password with every character class, never printed.
pub(crate) fn random_password() -> String {
    const SET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut password: String = random_bytes(28)
        .iter()
        .map(|b| char::from(SET[usize::from(*b) % SET.len()]))
        .collect();
    password.push_str("!7Qq");
    password
}

/// Win32 `Sleep`: the bounded retries' pause.
pub(crate) fn sleep_ms(ms: u32) {
    // SAFETY: Sleep only blocks this thread.
    unsafe { Sleep(ms) };
}

/// Milliseconds since boot (`GetTickCount64`): the bounded waits' clock.
pub(crate) fn tick_ms() -> u64 {
    // SAFETY: GetTickCount64 only reads the system's tick count.
    unsafe { GetTickCount64() }
}

/// Takes ownership of a handle a call just returned.
pub(crate) fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: the call succeeded, so `handle` is open, and nothing else holds it.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

/// The `HANDLE` of an owned handle, valid while it lives.
pub(crate) fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// A self-relative security descriptor made from SDDL, freed with `LocalFree`.
pub(crate) struct Sd(PSECURITY_DESCRIPTOR);

impl Sd {
    /// The descriptor `sddl` describes.
    pub(crate) fn from_sddl(sddl: &str) -> Res<Sd> {
        let text = wide(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` gets a
        // LocalAlloc'd descriptor, freed once by Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|e| failed("ConvertStringSecurityDescriptorToSecurityDescriptorW", &e))?;
        Ok(Sd(descriptor))
    }

    /// The descriptor's pointer, valid while `self` lives.
    pub(crate) fn ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.0
    }

    /// Its DACL, valid while `self` lives.
    pub(crate) fn dacl(&self) -> Res<*const ACL> {
        dacl_of(self.0)
    }

    /// Its SACL (the label), valid while `self` lives.
    pub(crate) fn sacl(&self) -> Res<*const ACL> {
        let (mut present, mut defaulted) = (BOOL(0), BOOL(0));
        let mut sacl: *mut ACL = std::ptr::null_mut();
        // SAFETY: the descriptor is valid while `self` lives; the outputs are locals.
        unsafe {
            GetSecurityDescriptorSacl(self.0, &raw mut present, &raw mut sacl, &raw mut defaulted)
        }
        .map_err(|e| failed("GetSecurityDescriptorSacl", &e))?;
        Ok(sacl.cast_const())
    }

    /// Attributes that create an object with this descriptor, not inheritable. Valid while
    /// `self` lives.
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Sd {
    fn drop(&mut self) {
        // SAFETY: the pointer came from the SDDL conversion (LocalAlloc) and is freed only here;
        // a failure leaks a few bytes, nothing to undo.
        let _leak_on_failure: HLOCAL = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// A self-relative descriptor read from an object, in an 8-byte aligned buffer.
pub(crate) struct SdBuf {
    /// The storage.
    words: Vec<u64>,
    /// The descriptor's length in bytes, as the read reported it.
    used: u32,
}

impl SdBuf {
    /// A zeroed buffer for a descriptor of `bytes` bytes.
    pub(crate) fn new(bytes: u32) -> SdBuf {
        let words = usize::try_from(bytes).unwrap_or(0).div_ceil(8);
        SdBuf {
            words: vec![0_u64; words.max(1)],
            used: bytes,
        }
    }

    /// The descriptor's length in bytes.
    pub(crate) fn len(&self) -> u32 {
        self.used
    }

    /// Its pointer for a call that fills it.
    pub(crate) fn ptr(&mut self) -> PSECURITY_DESCRIPTOR {
        PSECURITY_DESCRIPTOR(self.words.as_mut_ptr().cast::<c_void>())
    }

    /// Its pointer for a call that only reads it.
    pub(crate) fn read_ptr(&self) -> PSECURITY_DESCRIPTOR {
        PSECURITY_DESCRIPTOR(self.words.as_ptr().cast_mut().cast::<c_void>())
    }

    /// The descriptor's bytes.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let used = usize::try_from(self.used).unwrap_or(usize::MAX);
        self.words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .take(used)
            .collect()
    }
}

/// The DACL of `descriptor`, valid while the descriptor lives.
pub(crate) fn dacl_of(descriptor: PSECURITY_DESCRIPTOR) -> Res<*const ACL> {
    let (mut present, mut defaulted) = (BOOL(0), BOOL(0));
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: the caller's descriptor is valid; the outputs are locals.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        )
    }
    .map_err(|e| failed("GetSecurityDescriptorDacl", &e))?;
    Ok(dacl.cast_const())
}

/// The SDDL of the parts `info` names of `descriptor`. It may hold SIDs: compare it, never
/// print it.
pub(crate) fn sddl_of(
    descriptor: PSECURITY_DESCRIPTOR,
    info: OBJECT_SECURITY_INFORMATION,
) -> Res<String> {
    let mut text = PWSTR::null();
    // SAFETY: the caller's descriptor is valid; `text` gets a LocalAlloc'd string, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            info,
            &raw mut text,
            None,
        )
    }
    .map_err(|e| failed("ConvertSecurityDescriptorToStringSecurityDescriptorW", &e))?;
    local_string(text)
}

/// The text of a SID.
pub(crate) fn sid_text(sid: PSID) -> Res<String> {
    let mut text = PWSTR::null();
    // SAFETY: the caller's SID is valid; `text` gets a LocalAlloc'd string, freed below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }
        .map_err(|e| failed("ConvertSidToStringSidW", &e))?;
    local_string(text)
}

/// A `LocalAlloc`'d string a call returned, read and freed.
fn local_string(text: PWSTR) -> Res<String> {
    // SAFETY: the call that made `text` succeeded, so it is NUL-terminated.
    let string = unsafe { text.to_string() };
    // SAFETY: `text` came from LocalAlloc and is freed once, here.
    let _leak_on_failure: HLOCAL = unsafe { LocalFree(Some(HLOCAL(text.0.cast::<c_void>()))) };
    string.map_err(|e| format!("a string from Windows is not UTF-16: {e}"))
}

/// The SID text of this process's user (the owner).
pub(crate) fn token_user_sid() -> Res<String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process; `token` gets a new handle, owned below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
        .map_err(|e| failed("OpenProcessToken", &e))?;
    let token = owned(token);
    let mut buffer = vec![0_u64; 64];
    let mut needed = 0_u32;
    // SAFETY: `buffer` is writable and 8-byte aligned for 512 bytes and outlives the call.
    unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            512,
            &raw mut needed,
        )
    }
    .map_err(|e| failed("GetTokenInformation(TokenUser)", &e))?;
    // SAFETY: the call succeeded, so the buffer starts with a TOKEN_USER whose SID points into
    // the same live buffer.
    let sid = unsafe { buffer.as_ptr().cast::<TOKEN_USER>().read() }
        .User
        .Sid;
    sid_text(sid)
}
