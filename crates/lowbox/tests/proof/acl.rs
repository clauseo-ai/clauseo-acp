//! The file and device descriptors the row changes: the test root's protected DACL, the package's
//! grants on its folders, the traverse ACE on the system drive's root and the package's ACE on
//! `\Device\Null` (recipe step 3). The two machine-wide ones are saved first, written without
//! tree propagation (`SetFileSecurityW`, `SetKernelObjectSecurity`), put back from the saved
//! bytes, then read again and their ACE lists compared (not the DACL's flags, which NTFS may
//! normalise, such as `AI`). SDDL here holds SIDs, so it is compared, never printed.
#![expect(
    unsafe_code,
    reason = "FFI to CreateDirectoryW, CreateFileW and the file and object security calls"
)]

use std::os::windows::io::OwnedHandle;
use std::path::Path;

use windows::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetFileSecurityW, GetKernelObjectSecurity, SetFileSecurityW,
    SetKernelObjectSecurity, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
};
use windows::core::{BOOL, PCWSTR, w};

use super::win::{Res, Sd, SdBuf, failed, failed_last, owned, raw, sddl_of, wide};

/// The traverse ACE's mask on the drive's root: traverse, read attributes, synchronize.
const TRAVERSE: u32 = 0x0010_00A0;
/// The `\Device\Null` ACE's mask, as the recipe set it (`0x1201bf`).
const NULL_RIGHTS: u32 = 0x0012_01BF;
/// icacls `M` for the test account on the test root.
const MODIFY: u32 = 0x0013_01BF;

/// Makes the test root `path` with a protected DACL: SYSTEM and Administrators all access, the
/// test account `user` modify, each inherited by files and folders.
pub(crate) fn create_root(path: &Path, user: &str) -> Res<()> {
    let sd = Sd::from_sddl(&format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;{MODIFY:#x};;;{user})"
    ))?;
    let attributes = sd.attributes();
    let name = wide(path);
    // SAFETY: a NUL-terminated path and attributes whose descriptor outlives the call.
    unsafe { CreateDirectoryW(PCWSTR(name.as_ptr()), Some(&raw const attributes)) }
        .map_err(|e| failed("CreateDirectoryW(test root)", &e))
}

/// Makes the folder `path`, which inherits its parent's ACEs.
pub(crate) fn mkdir(path: &Path) -> Res<()> {
    let name = wide(path);
    // SAFETY: a NUL-terminated path; no attributes.
    unsafe { CreateDirectoryW(PCWSTR(name.as_ptr()), None) }
        .map_err(|e| failed("CreateDirectoryW", &e))
}

/// Adds the allow ACE `mask` for `sid` on the folder `path`, inherited by files and folders, on
/// top of what it inherits (`SetNamedSecurityInfoW` keeps the inherited ACEs of an unprotected
/// DACL). The folder is empty, so nothing below it needs the change.
pub(crate) fn grant(path: &Path, mask: u32, sid: &str) -> Res<()> {
    let sd = Sd::from_sddl(&format!("D:(A;OICI;{mask:#x};;;{sid})"))?;
    let dacl = sd.dacl()?;
    let name = wide(path);
    // SAFETY: a NUL-terminated path and a DACL that lives in `sd` until after the call.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(name.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
    };
    if rc.0 != 0 {
        return Err(format!("SetNamedSecurityInfoW(grant) failed with {}", rc.0));
    }
    Ok(())
}

/// `sddl` (a DACL alone, `D:<flags>(ace)…`) with `ace` first (`first`) or last.
fn with_ace(sddl: &str, ace: &str, first: bool) -> Res<String> {
    if !sddl.starts_with("D:") {
        return Err("the saved descriptor's SDDL has no DACL".to_owned());
    }
    let at = if first {
        sddl.find('(').unwrap_or(sddl.len())
    } else {
        sddl.len()
    };
    Ok(format!("{}{ace}{}", &sddl[..at], &sddl[at..]))
}

/// The ACE list of a DACL's SDDL (`D:<flags>(ace)…`): everything from the first ACE on.
fn aces(sddl: &str) -> &str {
    sddl.find('(').map_or("", |at| &sddl[at..])
}

/// The flags of a DACL's SDDL (such as `PAI`), which hold no SID.
fn flags(sddl: &str) -> &str {
    let rest = sddl.strip_prefix("D:").unwrap_or(sddl);
    &rest[..rest.find('(').unwrap_or(rest.len())]
}

/// Compares the re-read DACL's ACE list with the saved one's; the sentence for the log.
fn compare(saved: &str, now: &str) -> Res<String> {
    if aces(now) != aces(saved) {
        return Err("the re-read DACL's ACE list differs from the saved one".to_owned());
    }
    Ok(format!(
        "restored; the re-read DACL's ACE list equals the saved one (flags saved {:?}, now {:?})",
        flags(saved),
        flags(now)
    ))
}

/// Whether the re-read `now` holds `sid` while the saved `before` did not.
fn gained(before: &str, now: &str, sid: &str) -> bool {
    let sid = sid.to_ascii_lowercase();
    !before.to_ascii_lowercase().contains(&sid) && now.to_ascii_lowercase().contains(&sid)
}

/// The DACL of the file or folder `name` (NUL-terminated).
fn file_dacl(name: &[u16]) -> Res<SdBuf> {
    let mut needed = 0_u32;
    // SAFETY: the size query: no buffer, `needed` gets the bytes.
    let _sized: BOOL = unsafe {
        GetFileSecurityW(
            PCWSTR(name.as_ptr()),
            DACL_SECURITY_INFORMATION.0,
            None,
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(failed_last("GetFileSecurityW (size)"));
    }
    let mut buffer = SdBuf::new(needed);
    // SAFETY: `buffer` holds at least `needed` bytes, 8-byte aligned, and outlives the call.
    let ok = unsafe {
        GetFileSecurityW(
            PCWSTR(name.as_ptr()),
            DACL_SECURITY_INFORMATION.0,
            Some(buffer.ptr()),
            needed,
            &raw mut needed,
        )
    };
    if !ok.as_bool() {
        return Err(failed_last("GetFileSecurityW"));
    }
    Ok(buffer)
}

/// Writes the DACL of `descriptor` on the file or folder `name`, without propagation.
fn set_file_dacl(
    name: &[u16],
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
) -> Res<()> {
    // SAFETY: a NUL-terminated name and a valid descriptor, both outliving the call.
    let ok =
        unsafe { SetFileSecurityW(PCWSTR(name.as_ptr()), DACL_SECURITY_INFORMATION, descriptor) };
    if !ok.as_bool() {
        return Err(failed_last("SetFileSecurityW"));
    }
    Ok(())
}

/// The drive's root with the package's traverse ACE added; [`DriveRoot::restore`] puts the
/// saved DACL back.
pub(crate) struct DriveRoot {
    /// The root's NUL-terminated path, such as `C:\`.
    name: Vec<u16>,
    /// The DACL as it was.
    saved: SdBuf,
    /// Its SDDL, for the comparison after the restore.
    saved_sddl: String,
}

impl DriveRoot {
    /// Saves the DACL of `root` (such as `C:\`) and adds `(A;;0x1000a0;;;<package>)`, not
    /// inherited. The caller keeps the result for the restore, then calls `check`.
    pub(crate) fn add(root: &str, package: &str) -> Res<DriveRoot> {
        let name = wide(root);
        let saved = file_dacl(&name)?;
        let saved_sddl = sddl_of(saved.read_ptr(), DACL_SECURITY_INFORMATION)?;
        let target = with_ace(
            &saved_sddl,
            &format!("(A;;{TRAVERSE:#x};;;{package})"),
            false,
        )?;
        let changed = DriveRoot {
            name,
            saved,
            saved_sddl,
        };
        let sd = Sd::from_sddl(&target)?;
        set_file_dacl(&changed.name, sd.ptr())?;
        Ok(changed)
    }

    /// Reads the DACL again: it must now hold the package's ACE.
    pub(crate) fn check(&self, package: &str) -> Res<()> {
        let now = sddl_of(file_dacl(&self.name)?.read_ptr(), DACL_SECURITY_INFORMATION)?;
        if !gained(&self.saved_sddl, &now, package) {
            return Err("the drive root's re-read DACL does not show the package's ACE".to_owned());
        }
        Ok(())
    }

    /// Writes the saved DACL back, then reads it again and compares.
    pub(crate) fn restore(self) -> Res<String> {
        set_file_dacl(&self.name, self.saved.read_ptr())?;
        let now = file_dacl(&self.name)?;
        let now_sddl = sddl_of(now.read_ptr(), DACL_SECURITY_INFORMATION)?;
        let same = compare(&self.saved_sddl, &now_sddl)?;
        let same_bytes = now.bytes() == self.saved.bytes();
        Ok(format!(
            "{same}; bytes equal: {same_bytes}, {} bytes",
            self.saved.len()
        ))
    }
}

/// `\Device\Null` with the package's ACE prepended; [`NullDevice::restore`] puts the saved DACL
/// back.
pub(crate) struct NullDevice {
    /// `\\.\NUL` opened with `READ_CONTROL | WRITE_DAC`: its security is the device object's.
    handle: OwnedHandle,
    /// The DACL as it was.
    saved: SdBuf,
    /// Its SDDL, for the comparison after the restore.
    saved_sddl: String,
}

impl NullDevice {
    /// Saves the device's DACL and prepends `(A;;0x1201bf;;;<package>)`. The caller keeps the
    /// result for the restore, then calls `check`.
    pub(crate) fn add(package: &str) -> Res<NullDevice> {
        // SAFETY: a constant NUL-terminated name; no attributes; the handle is owned below.
        let handle = unsafe {
            CreateFileW(
                w!(r"\\.\NUL"),
                READ_CONTROL.0 | WRITE_DAC.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .map_err(|e| failed(r"CreateFileW(\\.\NUL, READ_CONTROL | WRITE_DAC)", &e))?;
        let handle = owned(handle);
        let saved = object_dacl(&handle)?;
        let saved_sddl = sddl_of(saved.read_ptr(), DACL_SECURITY_INFORMATION)?;
        let target = with_ace(
            &saved_sddl,
            &format!("(A;;{NULL_RIGHTS:#x};;;{package})"),
            true,
        )?;
        let changed = NullDevice {
            handle,
            saved,
            saved_sddl,
        };
        let sd = Sd::from_sddl(&target)?;
        set_object_dacl(&changed.handle, sd.ptr())?;
        Ok(changed)
    }

    /// Reads the DACL again: it must now hold the package's ACE.
    pub(crate) fn check(&self, package: &str) -> Res<()> {
        let now = sddl_of(
            object_dacl(&self.handle)?.read_ptr(),
            DACL_SECURITY_INFORMATION,
        )?;
        if !gained(&self.saved_sddl, &now, package) {
            return Err(r"\Device\Null's re-read DACL does not show the package's ACE".to_owned());
        }
        Ok(())
    }

    /// Writes the saved DACL back, then reads it again and compares.
    pub(crate) fn restore(self) -> Res<String> {
        set_object_dacl(&self.handle, self.saved.read_ptr())?;
        let now = object_dacl(&self.handle)?;
        let now_sddl = sddl_of(now.read_ptr(), DACL_SECURITY_INFORMATION)?;
        let same = compare(&self.saved_sddl, &now_sddl)?;
        let same_bytes = now.bytes() == self.saved.bytes();
        Ok(format!("{same}; bytes equal: {same_bytes}"))
    }
}

/// The DACL of the object behind `handle`.
fn object_dacl(handle: &OwnedHandle) -> Res<SdBuf> {
    let mut needed = 0_u32;
    // SAFETY: the size query on an open handle: no buffer, `needed` gets the bytes. It fails
    // with ERROR_INSUFFICIENT_BUFFER by design.
    let _sized: windows::core::Result<()> = unsafe {
        GetKernelObjectSecurity(
            raw(handle),
            DACL_SECURITY_INFORMATION.0,
            None,
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(failed_last("GetKernelObjectSecurity (size)"));
    }
    let mut buffer = SdBuf::new(needed);
    // SAFETY: `buffer` holds at least `needed` bytes, 8-byte aligned, and outlives the call.
    unsafe {
        GetKernelObjectSecurity(
            raw(handle),
            DACL_SECURITY_INFORMATION.0,
            Some(buffer.ptr()),
            needed,
            &raw mut needed,
        )
    }
    .map_err(|e| failed("GetKernelObjectSecurity", &e))?;
    Ok(buffer)
}

/// Writes the DACL of `descriptor` on the object behind `handle`.
fn set_object_dacl(
    handle: &OwnedHandle,
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
) -> Res<()> {
    // SAFETY: an open handle with WRITE_DAC and a valid descriptor, both outliving the call.
    unsafe { SetKernelObjectSecurity(raw(handle), DACL_SECURITY_INFORMATION, descriptor) }
        .map_err(|e| failed("SetKernelObjectSecurity", &e))
}
