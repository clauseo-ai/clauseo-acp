//! The Windows-only unit tests: the package SID against the API, and the handle checks against
//! real handles of this process. No namespace is built here (that needs the proof row).
#![expect(
    unsafe_code,
    reason = "FFI to DeriveAppContainerSidFromAppContainerName and FreeSid"
)]

use std::os::windows::io::AsRawHandle;

use ::windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use ::windows::Win32::Security::FreeSid;
use ::windows::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use ::windows::core::{HSTRING, PWSTR};

use crate::{Error, HandleProblem, NamespaceHandles, PackageName, Slot, package_sid};

/// The derivation matches `DeriveAppContainerSidFromAppContainerName`, which needs no profile.
#[test]
fn the_package_sid_matches_the_api() {
    for name in ["Clauseo.Browser.B01", "Clauseo.Browser.B02", "x", "A-b.C"] {
        let package = PackageName::new(name).unwrap();
        // SAFETY: a NUL-terminated name; the SID it returns is freed with FreeSid below.
        let sid =
            unsafe { DeriveAppContainerSidFromAppContainerName(&HSTRING::from(name)) }.unwrap();
        let mut text = PWSTR::null();
        // SAFETY: `sid` is valid until FreeSid; `text` gets a string leaked by this test.
        unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.unwrap();
        // SAFETY: the call succeeded, so `text` is NUL-terminated.
        let api = unsafe { text.to_string() }.unwrap();
        // SAFETY: the SID came from DeriveAppContainerSidFromAppContainerName.
        unsafe { FreeSid(sid) };
        assert_eq!(package_sid(&package).as_str(), api, "{name}");
    }
}

fn b01() -> PackageName {
    PackageName::new("Clauseo.Browser.B01").unwrap()
}

#[test]
fn a_value_that_is_no_handle_fails_closed() {
    // Handle values are multiples of 4; this one is far past any table this test makes.
    let values = [
        0x00ff_fff0,
        0x00ff_fff4,
        0x00ff_fff8,
        0x00ff_fffc,
        0x00ff_ffe0,
        0x00ff_ffe4,
    ];
    let error = NamespaceHandles::from_inherited(&b01(), &values).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Handle {
                slot: Slot::Directory,
                problem: HandleProblem::NotOpen { .. }
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_file_in_the_directory_slot_fails_closed() {
    let files: Vec<std::fs::File> = (0..6)
        .map(|_| std::fs::File::open(std::env::current_exe().unwrap()).unwrap())
        .collect();
    let values: Vec<usize> = files
        .iter()
        .map(|f| f.as_raw_handle().expose_provenance())
        .collect();
    let error = NamespaceHandles::from_inherited(&b01(), &values).unwrap_err();
    assert_eq!(
        error,
        Error::Handle {
            slot: Slot::Directory,
            problem: HandleProblem::WrongType {
                found: "File".to_owned()
            }
        }
    );
}
