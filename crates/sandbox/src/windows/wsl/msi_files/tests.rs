//! The chain check on real files, unelevated: the policy's flags, a Microsoft file chaining to
//! Root 2011 (Edge), one chaining to Root 2010 (`kernel32.dll`), and a binary signed by
//! someone else (Git for Windows), each skipped with a message where the file is absent.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;

use windows::Win32::Security::Cryptography::{
    CERT_CHAIN_POLICY_PARA, MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG,
    MICROSOFT_ROOT_CERT_CHAIN_POLICY_ENABLE_TEST_ROOT_FLAG,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING,
};
use windows::core::HSTRING;

use super::{policy_para, signature};
use crate::windows::wsl::msi_verify::Signature;

/// Edge, signed through Microsoft Code Signing PCA 2024 to Root 2011 on current builds.
const EDGE: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
/// Git for Windows, signed by Johannes Schindelin, on GitHub's Windows runners.
const GIT: &str = r"C:\Program Files\Git\bin\git.exe";

/// `path` for reading, or `None` with a message when it isn't there.
fn open(path: &str) -> Option<OwnedHandle> {
    if !Path::new(path).is_file() {
        eprintln!("skipped: {path} isn't on this machine");
        return None;
    }
    let name = HSTRING::from(path);
    // SAFETY: `name` is NUL-terminated and outlives the call; the handle is owned at once.
    let handle = unsafe {
        CreateFileW(
            &name,
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .unwrap_or_else(|error| panic!("{path}: {error}"));
    // SAFETY: CreateFileW returned a valid handle that nothing else owns.
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

#[test]
fn the_policy_takes_the_application_root_and_never_the_test_root() {
    let para = policy_para();
    assert_eq!(para.dwFlags.0 & 0x2_0000, 0x2_0000);
    assert_eq!(
        MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG.0,
        0x2_0000
    );
    assert_eq!(
        para.dwFlags,
        MICROSOFT_ROOT_CERT_CHAIN_POLICY_CHECK_APPLICATION_ROOT_FLAG
    );
    assert_eq!(
        MICROSOFT_ROOT_CERT_CHAIN_POLICY_ENABLE_TEST_ROOT_FLAG.0,
        0x1_0000
    );
    assert_eq!(para.dwFlags.0 & 0x1_0000, 0);
    let size = u32::try_from(size_of::<CERT_CHAIN_POLICY_PARA>()).unwrap();
    assert_eq!(para.cbSize, size);
    assert!(para.pvExtraPolicyPara.is_null());
}

#[test]
fn a_microsoft_file_chaining_to_root_2011_passes() {
    let Some(file) = open(EDGE) else { return };
    let signed = signature(&file, EDGE);
    let expected = Signature {
        microsoft_root: true,
        code_signing: true,
    };
    assert_eq!(signed, Ok(expected));
}

#[test]
fn kernel32_chaining_to_root_2010_passes_the_root_check() {
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_unset| r"C:\Windows".to_owned());
    let path = format!(r"{windows}\System32\kernel32.dll");
    let Some(file) = open(&path) else { return };
    match signature(&file, &path) {
        Ok(signed) => assert!(signed.microsoft_root, "{signed:?}"),
        // A catalog-signed build has no embedded signature for the file mode to read.
        Err(why) => eprintln!("skipped: {path} has no embedded signature here: {why}"),
    }
}

#[test]
fn a_binary_signed_by_someone_else_is_refused() {
    let Some(file) = open(GIT) else { return };
    match signature(&file, GIT) {
        Ok(signed) => assert!(!signed.microsoft_root, "{signed:?}"),
        Err(why) => assert!(why.contains("couldn't") || why.contains("refused"), "{why}"),
    }
}
