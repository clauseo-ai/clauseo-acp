//! Windows' checks of a DLL about to be placed beside Codex's `rg.exe`: its Authenticode signer
//! (`WinVerifyTrust`, then the signer's certificate and chain from its state data) and its
//! version resource (`GetFileVersionInfoW`). Both read the file and run none of it; nothing is
//! written. No UI is shown, and no revocation is fetched: `WTD_CACHE_ONLY_URL_RETRIEVAL` keeps
//! an offline or proxied machine from waiting on the network in the middle of an install.
#![expect(unsafe_code, reason = "FFI to wintrust, crypt32 and version.dll")]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{HANDLE, HWND, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Cryptography::{
    CERT_CHAIN_CONTEXT, CERT_CHAIN_POLICY_FLAGS, CERT_CHAIN_POLICY_MICROSOFT_ROOT,
    CERT_CHAIN_POLICY_PARA, CERT_CHAIN_POLICY_STATUS, CERT_CONTEXT, CERT_NAME_ATTR_TYPE,
    CertGetNameStringW, CertVerifyCertificateChainPolicy,
    MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG, szOID_ORGANIZATION_NAME,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::core::{PCWSTR, w};

use super::Signer;

/// The organisation Microsoft's code-signing certificates name.
const MICROSOFT: &str = "Microsoft Corporation";
/// `VS_FIXEDFILEINFO`'s `dwSignature`.
const FIXED_SIGNATURE: u32 = 0xFEEF_04BD;

/// `path` as a NUL-terminated UTF-16 string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// `size_of::<T>()` as the `u32` Win32's `cbStruct` fields take.
fn cb<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// The file's Authenticode signer: [`Signer::Microsoft`] only when `WinVerifyTrust` accepts its
/// embedded signature, the signer certificate's organisation is Microsoft Corporation, and the
/// chain ends at a Microsoft root.
pub(super) fn signer(path: &Path) -> Signer {
    let wide = wide(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: cb::<WINTRUST_FILE_INFO>(),
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: cb::<WINTRUST_DATA>(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut file,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` is a WINTRUST_DATA with its size set, choosing a WINTRUST_FILE_INFO
    // (`file`, sized) whose path is `wide`, NUL-terminated; all three outlive the call. The
    // INVALID_HANDLE_VALUE window asks for no UI.
    let status = unsafe {
        WinVerifyTrust(
            HWND(INVALID_HANDLE_VALUE.0),
            &raw mut action,
            (&raw mut data).cast::<c_void>(),
        )
    };
    let found = if status == 0 {
        // SAFETY: the verify succeeded, so `hWVTStateData` holds its state until the close below.
        unsafe { signer_of(data.hWVTStateData) }
    } else {
        Signer::NotTrusted(format!(
            "WinVerifyTrust 0x{:08X}",
            u32::from_ne_bytes(status.to_ne_bytes())
        ))
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: the same structures, still alive; the close frees the state the verify kept
    // (it is a no-op when the verify kept none).
    unsafe {
        WinVerifyTrust(
            HWND(INVALID_HANDLE_VALUE.0),
            &raw mut action,
            (&raw mut data).cast::<c_void>(),
        )
    };
    found
}

/// The signer found in a successful verify's state data.
///
/// # Safety
///
/// `state` is the `hWVTStateData` of a successful `WTD_STATEACTION_VERIFY` not yet closed.
unsafe fn signer_of(state: HANDLE) -> Signer {
    // SAFETY: `state` is live state data, as the caller promises.
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return Signer::NotTrusted("no provider data".to_owned());
    }
    // SAFETY: `provider` is the non-null provider data of that state; signer 0 is the primary.
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, false, 0) };
    // SAFETY: a non-null result points at the provider's signer, alive until the close.
    let Some(signer) = (unsafe { signer.as_ref() }) else {
        return Signer::NotTrusted("no signer".to_owned());
    };
    if signer.csCertChain == 0 || signer.pasCertChain.is_null() {
        return Signer::NotTrusted("no signer certificate".to_owned());
    }
    // SAFETY: `pasCertChain` holds `csCertChain` (at least one) entries; the first is the
    // signer's own certificate.
    let leaf = unsafe { (*signer.pasCertChain).pCert };
    if leaf.is_null() {
        return Signer::NotTrusted("no signer certificate".to_owned());
    }
    // SAFETY: `leaf` is a certificate context the state data owns, alive until the close.
    let organisation = unsafe { organisation(leaf) };
    // SAFETY: `pChainContext` is the signer's chain the state data owns, or null.
    let microsoft_root =
        !signer.pChainContext.is_null() && unsafe { microsoft_root(signer.pChainContext) };
    if organisation == MICROSOFT && microsoft_root {
        Signer::Microsoft
    } else {
        Signer::Other(format!(
            "organisation {organisation:?}, Microsoft root: {microsoft_root}"
        ))
    }
}

/// The certificate's subject organisation (`O=`), or empty.
///
/// # Safety
///
/// `cert` is a valid certificate context for the call's duration.
unsafe fn organisation(cert: *const CERT_CONTEXT) -> String {
    let oid = Some(szOID_ORGANIZATION_NAME.0.cast::<c_void>());
    // SAFETY: a valid context, the attribute type with an OID string that is 'static; no
    // buffer asks for the length, terminator included.
    let len = unsafe { CertGetNameStringW(cert, CERT_NAME_ATTR_TYPE, 0, oid, None) };
    let mut buffer = vec![0_u16; usize::try_from(len).unwrap_or(0)];
    if buffer.len() <= 1 {
        return String::new();
    }
    // SAFETY: as above, with a buffer of the length the first call gave.
    let written = unsafe {
        CertGetNameStringW(
            cert,
            CERT_NAME_ATTR_TYPE,
            0,
            oid,
            Some(buffer.as_mut_slice()),
        )
    };
    let end = usize::try_from(written)
        .unwrap_or(0)
        .saturating_sub(1)
        .min(buffer.len());
    String::from_utf16_lossy(buffer.get(..end).unwrap_or_default())
}

/// Whether the chain ends at a Microsoft root: `CERT_CHAIN_POLICY_MICROSOFT_ROOT`, with its
/// product roots, or with the application root ("Microsoft Root Certificate Authority 2011",
/// which the flag checks instead).
///
/// # Safety
///
/// `chain` is a valid chain context for the call's duration.
unsafe fn microsoft_root(chain: *const CERT_CHAIN_CONTEXT) -> bool {
    [
        CERT_CHAIN_POLICY_FLAGS(0),
        MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG,
    ]
    .into_iter()
    .any(|flags| {
        let para = CERT_CHAIN_POLICY_PARA {
            cbSize: cb::<CERT_CHAIN_POLICY_PARA>(),
            dwFlags: flags,
            pvExtraPolicyPara: std::ptr::null_mut(),
        };
        let mut status = CERT_CHAIN_POLICY_STATUS {
            cbSize: cb::<CERT_CHAIN_POLICY_STATUS>(),
            ..Default::default()
        };
        // SAFETY: a valid chain (the caller's promise), a sized PARA and a writable, sized
        // STATUS, all alive for the call.
        let ran = unsafe {
            CertVerifyCertificateChainPolicy(
                CERT_CHAIN_POLICY_MICROSOFT_ROOT,
                chain,
                &raw const para,
                &raw mut status,
            )
        };
        ran.as_bool() && status.dwError == 0
    })
}

/// The file's `FILEVERSION` from its version resource, major first; `None` without one.
pub(super) fn file_version(path: &Path) -> Option<[u16; 4]> {
    let wide = wide(path);
    let name = PCWSTR(wide.as_ptr());
    // SAFETY: `name` is NUL-terminated and outlives the call; the handle out-parameter is
    // optional.
    let size = unsafe { GetFileVersionInfoSizeW(name, None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0_u8; usize::try_from(size).ok()?];
    // SAFETY: `block` is writable for `size` bytes, as the call is told.
    unsafe { GetFileVersionInfoW(name, None, size, block.as_mut_ptr().cast::<c_void>()) }.ok()?;
    let mut value: *mut c_void = std::ptr::null_mut();
    let mut len = 0_u32;
    // SAFETY: `block` holds the version data just read and outlives `value`, which points
    // into it; `\` names the root's VS_FIXEDFILEINFO.
    let found = unsafe {
        VerQueryValueW(
            block.as_ptr().cast::<c_void>(),
            w!("\\"),
            &raw mut value,
            &raw mut len,
        )
    };
    if !found.as_bool()
        || value.is_null()
        || usize::try_from(len).ok()? < size_of::<VS_FIXEDFILEINFO>()
    {
        return None;
    }
    // SAFETY: `value` points at `len` (at least a VS_FIXEDFILEINFO's) bytes inside `block`;
    // read unaligned, since nothing promises its alignment.
    let info = unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    drop(block);
    if info.dwSignature != FIXED_SIGNATURE {
        return None;
    }
    let half = |word: u32, shift: u32| u16::try_from((word >> shift) & 0xFFFF).unwrap_or(0);
    Some([
        half(info.dwFileVersionMS, 16),
        half(info.dwFileVersionMS, 0),
        half(info.dwFileVersionLS, 16),
        half(info.dwFileVersionLS, 0),
    ])
}
