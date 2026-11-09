//! [`MsiFiles`] on the real file system, for the SYSTEM run's MSI (`msi_verify.rs`). Nothing
//! past `WindowsApps` is opened by its full path: `WindowsApps` is opened without following a
//! reparse point, then the package's folder relative to its handle, then `wsl.msi` relative to
//! the folder's (`NtCreateFile` with `RootDirectory`, `OBJ_DONT_REPARSE` and
//! `FILE_OPEN_REPARSE_POINT`, as `refresh/walk.rs` opens), each refused if a reparse point. The
//! folders are held without delete sharing, so neither can be renamed while the MSI is in use;
//! the MSI is opened for reading and shared for reading only, so it can't be written, renamed or
//! deleted until the handle closes, after msiexec has ended.
//!
//! The signature: `WinVerifyTrust` with `WINTRUST_ACTION_GENERIC_VERIFY_V2` and no UI, on the
//! held handle. Revocation isn't checked (`WTD_REVOKE_NONE`, `WTD_REVOCATION_CHECK_NONE`), and
//! `WTD_CACHE_ONLY_URL_RETRIEVAL` keeps chain building off the network, so an offline machine
//! verifies as an online one does. The signer's chain must then pass
//! `CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_MICROSOFT_ROOT)` with the application-root
//! flag ([`policy_para`]): it ends in Microsoft's application root, Root 2011, which a root an
//! administrator (or anyone) added can't stand in for; and its leaf certificate must
//! list the code-signing EKU (`szOID_PKIX_KP_CODE_SIGNING`).
#![expect(
    unsafe_code,
    reason = "FFI to CreateFileW, NtCreateFile, GetSecurityInfo and WinVerifyTrust"
)]

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SEQUENTIAL_ONLY,
    FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows::Win32::Foundation::{
    HANDLE, HLOCAL, HWND, INVALID_HANDLE_VALUE, LocalFree, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows::Win32::Security::Cryptography::{
    CERT_CHAIN_CONTEXT, CERT_CHAIN_POLICY_MICROSOFT_ROOT, CERT_CHAIN_POLICY_PARA,
    CERT_CHAIN_POLICY_STATUS, CTL_USAGE, CertGetEnhancedKeyUsage, CertVerifyCertificateChainPolicy,
    MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG, szOID_PKIX_KP_CODE_SIGNING,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ,
    FILE_READ_ATTRIBUTES, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    FILE_TYPE_DISK, FileAttributeTagInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetFileType, OPEN_EXISTING, SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::msi_verify::{MsiFiles, Signature};
use super::upgrade::PACKAGE_MSI;

/// What a folder's open asks for, and shares: no delete, so it can't be renamed while held.
const FOLDER_ACCESS: FILE_ACCESS_RIGHTS =
    FILE_ACCESS_RIGHTS(FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0);
const FOLDER_SHARE: FILE_SHARE_MODE = FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0);

/// The real file system.
pub(super) struct WindowsMsiFiles;

/// `WindowsApps`, the package's folder and `wsl.msi`, open from the checks to the install's end.
pub(super) struct HeldMsi {
    _apps: OwnedHandle,
    _package: OwnedHandle,
    file: OwnedHandle,
}

impl MsiFiles for WindowsMsiFiles {
    type Held = HeldMsi;

    fn open(&self, apps: &str, package: &str) -> Result<HeldMsi, String> {
        let apps_handle = open_apps(apps)?;
        attributes(&apps_handle, "WindowsApps", true)?;
        let package_handle = open_at(&apps_handle, package, true)?;
        attributes(&package_handle, "the package's folder", true)?;
        let file = open_at(&package_handle, PACKAGE_MSI, false)?;
        attributes(&file, PACKAGE_MSI, false)?;
        regular(&file)?;
        Ok(HeldMsi {
            _apps: apps_handle,
            _package: package_handle,
            file,
        })
    }

    fn owner(&self, msi: &HeldMsi) -> Result<String, String> {
        owner(&msi.file)
    }

    fn signature(&self, msi: &HeldMsi, path: &str) -> Result<Signature, String> {
        signature(&msi.file, path)
    }
}

/// `HANDLE` of an owned handle, for the calls that take one.
fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// `apps`, by its path, for traversal, not followed if a reparse point.
fn open_apps(apps: &str) -> Result<OwnedHandle, String> {
    let name = HSTRING::from(apps);
    // SAFETY: `name` is NUL-terminated and outlives the call; the handle is owned at once.
    let handle = unsafe {
        CreateFileW(
            &name,
            FOLDER_ACCESS.0,
            FOLDER_SHARE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| format!("WindowsApps couldn't be opened: {}", code(&error)))?;
    // SAFETY: CreateFileW returned a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// `name` (one path part) in `parent`, relative to its handle: `OBJ_DONT_REPARSE` fails the
/// open on any reparse point met, and `FILE_OPEN_REPARSE_POINT` opens one at the name itself,
/// which [`attributes`] refuses. A file is opened for reading, shared for reading only, with no
/// kind named, so a junction at its name opens as itself and is refused by its attributes.
fn open_at(parent: &OwnedHandle, name: &str, folder: bool) -> Result<OwnedHandle, String> {
    let what = if folder {
        "the package's folder"
    } else {
        PACKAGE_MSI
    };
    let units: Vec<u16> = name.encode_utf16().collect();
    let bytes = u16::try_from(units.len() * 2).map_err(|_too_long| format!("{what}: too long"))?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: raw(parent),
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let (access, share, options) = if folder {
        (FOLDER_ACCESS, FOLDER_SHARE, FILE_DIRECTORY_FILE)
    } else {
        (FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SEQUENTIAL_ONLY)
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: `unicode` points into `units`, which outlives the call and is never written;
    // `attributes` and the out-pointers are live; the parent's handle is open.
    let done = unsafe {
        NtCreateFile(
            &raw mut handle,
            access,
            &raw const attributes,
            &raw mut status,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            share,
            FILE_OPEN,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if done.is_err() {
        // SAFETY: RtlNtStatusToDosError only maps a number.
        let code = unsafe { RtlNtStatusToDosError(done) };
        return Err(format!("{what} couldn't be opened: {code}"));
    }
    // SAFETY: NtCreateFile succeeded, so `handle` is valid and nothing else owns it.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// Refuses an open `what` that is a reparse point, or isn't a folder (`folder`) or is one.
fn attributes(handle: &OwnedHandle, what: &str, folder: bool) -> Result<(), String> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open with FILE_READ_ATTRIBUTES and `tag` is valid and writable for
    // its size.
    unsafe {
        GetFileInformationByHandleEx(
            raw(handle),
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|error| format!("{what}'s attributes: {}", code(&error)))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || tag.ReparseTag != 0 {
        let reparse = tag.ReparseTag;
        return Err(format!("{what} is a reparse point (tag {reparse:#010X})"));
    }
    let is_folder = tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    match (folder, is_folder) {
        (true, false) => Err(format!("{what} isn't a folder")),
        (false, true) => Err(format!("{what} is a folder")),
        _ => Ok(()),
    }
}

/// Refuses the MSI unless a disk file with one link.
fn regular(file: &OwnedHandle) -> Result<(), String> {
    // SAFETY: the handle is open.
    if unsafe { GetFileType(raw(file)) } != FILE_TYPE_DISK {
        return Err(format!("{PACKAGE_MSI} isn't a disk file"));
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `info` is a live out-pointer.
    unsafe { GetFileInformationByHandle(raw(file), &raw mut info) }
        .map_err(|error| format!("{PACKAGE_MSI}'s links: {}", code(&error)))?;
    match info.nNumberOfLinks {
        1 => Ok(()),
        links => Err(format!("{PACKAGE_MSI} has {links} links")),
    }
}

/// The open file's owner, a string SID, read on its handle (opened with `READ_CONTROL`).
fn owner(file: &OwnedHandle) -> Result<String, String> {
    let mut sid = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open; both out-pointers are live; `sid` points into the
    // descriptor, freed below after the SID is read.
    let status = unsafe {
        GetSecurityInfo(
            raw(file),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut sid),
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if status.is_err() {
        return Err(format!("{PACKAGE_MSI}'s owner: {}", status.0));
    }
    let mut text = PWSTR::null();
    // SAFETY: `sid` is a valid SID inside the live descriptor; `text` is a live out-pointer,
    // freed with LocalFree below.
    let converted = unsafe { ConvertSidToStringSidW(sid, &raw mut text) };
    let owner = converted
        .map_err(|error| format!("{PACKAGE_MSI}'s owner: {}", code(&error)))
        .and_then(|()| {
            // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
            unsafe { text.to_string() }.map_err(|_bad| format!("{PACKAGE_MSI}'s owner: not text"))
        });
    // SAFETY: each was allocated with LocalAlloc by the calls above (or is null), freed once.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    owner
}

/// The open file's signature, once `WinVerifyTrust` trusts it, as the module says.
pub(super) fn signature(file: &OwnedHandle, path: &str) -> Result<Signature, String> {
    let wide = HSTRING::from(path);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: u32::try_from(size_of::<WINTRUST_FILE_INFO>()).unwrap_or(0),
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: raw(file),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: u32::try_from(size_of::<WINTRUST_DATA>()).unwrap_or(0),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_NONE | WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_ui = HWND(INVALID_HANDLE_VALUE.0);
    // SAFETY: `data`, `info`, `wide` and `action` are live across the call; the handle is open.
    let status =
        unsafe { WinVerifyTrust(no_ui, &raw mut action, (&raw mut data).cast::<c_void>()) };
    let verdict = if status == 0 {
        verdict(data.hWVTStateData)
    } else {
        let status = status.cast_unsigned();
        Err(format!(
            "WinVerifyTrust refused {PACKAGE_MSI}: {status:#010X}"
        ))
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: as above; this frees the state the first call kept.
    unsafe { WinVerifyTrust(no_ui, &raw mut action, (&raw mut data).cast::<c_void>()) };
    verdict
}

/// The `CERT_CHAIN_POLICY_MICROSOFT_ROOT` parameters: Microsoft's application root, Microsoft
/// Root Certificate Authority 2011, which the flag checks instead of the product roots; never the
/// test root. `wsl.msi` chains Microsoft Corporation, Microsoft Code Signing PCA 2024, then Root
/// 2011, which flag 0 doesn't accept (`0x800B0109`, `CERT_E_UNTRUSTEDROOT`, on the VM); a chain
/// to Root 2010, as `kernel32.dll`'s embedded signature, fails with the flag (CI run 42520974059).
pub(super) fn policy_para() -> CERT_CHAIN_POLICY_PARA {
    CERT_CHAIN_POLICY_PARA {
        cbSize: u32::try_from(size_of::<CERT_CHAIN_POLICY_PARA>()).unwrap_or(0),
        dwFlags: MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG,
        ..CERT_CHAIN_POLICY_PARA::default()
    }
}

/// The first signer's chain in `state`: whether it passes `CERT_CHAIN_POLICY_MICROSOFT_ROOT`
/// with [`policy_para`], and whether its leaf is for code signing.
fn verdict(state: HANDLE) -> Result<Signature, String> {
    let none = || format!("{PACKAGE_MSI}'s signer couldn't be read");
    // SAFETY: `state` is WinVerifyTrust's live state, closed by the caller after this returns;
    // each pointer is checked for null before it is used and is owned by that state.
    let chain = unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return Err(none());
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, false, 0);
        if signer.is_null() {
            return Err(none());
        }
        (*signer).pChainContext
    };
    if chain.is_null() {
        return Err(none());
    }
    let para = policy_para();
    let mut status = CERT_CHAIN_POLICY_STATUS {
        cbSize: u32::try_from(size_of::<CERT_CHAIN_POLICY_STATUS>()).unwrap_or(0),
        ..CERT_CHAIN_POLICY_STATUS::default()
    };
    // SAFETY: `chain` is a live chain in the state; `para` and `status` are live for the call.
    let checked = unsafe {
        CertVerifyCertificateChainPolicy(
            CERT_CHAIN_POLICY_MICROSOFT_ROOT,
            chain,
            &raw const para,
            &raw mut status,
        )
    };
    if !checked.as_bool() {
        return Err(format!("{PACKAGE_MSI}'s chain policy couldn't be checked"));
    }
    // SAFETY: `chain` is live, as above.
    let code_signing = unsafe { leaf_code_signing(chain) }?;
    Ok(Signature {
        microsoft_root: status.dwError == 0,
        code_signing,
    })
}

/// Whether the leaf certificate of `chain`'s first simple chain lists the code-signing EKU
/// (`szOID_PKIX_KP_CODE_SIGNING`). One with no EKU extension (valid for every use) doesn't.
///
/// # Safety
///
/// `chain` must be a live chain context.
unsafe fn leaf_code_signing(chain: *const CERT_CHAIN_CONTEXT) -> Result<bool, String> {
    let none = || format!("{PACKAGE_MSI}'s signer couldn't be read");
    // SAFETY: the caller's chain is live; each count is checked before its array is read.
    let leaf = unsafe {
        if (*chain).cChain == 0 {
            return Err(none());
        }
        let simple = *(*chain).rgpChain;
        if simple.is_null() || (*simple).cElement == 0 {
            return Err(none());
        }
        let element = *(*simple).rgpElement;
        if element.is_null() {
            return Err(none());
        }
        (*element).pCertContext
    };
    if leaf.is_null() {
        return Err(none());
    }
    let mut size = 0u32;
    // SAFETY: a live certificate; a size query first, with no buffer.
    unsafe { CertGetEnhancedKeyUsage(leaf, 0, None, &raw mut size) }.map_err(|_failed| none())?;
    let len = usize::try_from(size).map_err(|_too_big| none())?;
    let words = len.div_ceil(size_of::<usize>()).max(1);
    // A `usize` buffer, so the `CTL_USAGE` the call writes at its start is aligned.
    let mut buffer = vec![0usize; words];
    let usage = buffer.as_mut_ptr().cast::<CTL_USAGE>();
    // SAFETY: `buffer` holds at least `size` bytes, aligned for `CTL_USAGE`.
    unsafe { CertGetEnhancedKeyUsage(leaf, 0, Some(usage), &raw mut size) }
        .map_err(|_failed| none())?;
    // SAFETY: the call filled `usage` and the identifiers it points to, inside `buffer`.
    let found = unsafe {
        let count = usize::try_from((*usage).cUsageIdentifier).unwrap_or(0);
        let ids = (*usage).rgpszUsageIdentifier;
        (0..count).any(|at| {
            let id = *ids.add(at);
            !id.is_null() && id.as_bytes() == szOID_PKIX_KP_CODE_SIGNING.as_bytes()
        })
    };
    Ok(found)
}

/// An error's Win32 code.
fn code(error: &windows::core::Error) -> u32 {
    u32::try_from(error.code().0 & 0xFFFF).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
