//! The Windows-only unit tests: the package SID against the API, and the handle checks against
//! real handles of this process. No namespace is built here (that needs the proof row).
#![expect(
    unsafe_code,
    reason = "FFI to DeriveAppContainerSidFromAppContainerName and FreeSid"
)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

use ::windows::Win32::Foundation::{
    CompareObjectHandles, DUPLICATE_SAME_ACCESS, DuplicateHandle, GetHandleInformation, HANDLE,
    HANDLE_FLAG_INHERIT,
};
use ::windows::Win32::Security::SECURITY_ATTRIBUTES;
use ::windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, TerminateProcess};

use ::windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use ::windows::Win32::Security::FreeSid;
use ::windows::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use ::windows::core::{HSTRING, PWSTR};

use crate::{
    Error, HandleProblem, LowBoxCommand, NamespaceHandles, PackageName, Slot, Spawned, Stdio,
    package_sid,
};

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

/// A new unnamed event, inheritable or not.
fn event(inheritable: bool) -> OwnedHandle {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap(),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: inheritable.into(),
    };
    // SAFETY: the attributes outlive the call; the event is owned below.
    let handle = unsafe { CreateEventW(Some(&raw const attributes), true, false, None) }.unwrap();
    // SAFETY: CreateEventW succeeded, so the handle is open and ours alone.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

/// Whether the suspended `child` holds, at the value of `ours`, the same object.
fn child_holds(child: &Spawned, ours: &OwnedHandle) -> bool {
    let mut copy = HANDLE::default();
    // SAFETY: the child's process handle has all access; the copy is a new handle in this
    // process, owned below. A value the child does not hold makes the call fail.
    let copied = unsafe {
        DuplicateHandle(
            HANDLE(child.process.as_raw_handle()),
            HANDLE(ours.as_raw_handle()),
            GetCurrentProcess(),
            &raw mut copy,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if copied.is_err() {
        return false;
    }
    // SAFETY: DuplicateHandle succeeded, so `copy` is open and ours alone.
    let copy = unsafe { OwnedHandle::from_raw_handle(copy.0) };
    // SAFETY: both handles are open in this process.
    unsafe { CompareObjectHandles(HANDLE(copy.as_raw_handle()), HANDLE(ours.as_raw_handle())) }
        .as_bool()
}

/// Whether `handle` has `HANDLE_FLAG_INHERIT`.
fn inheritable(handle: &OwnedHandle) -> bool {
    let mut flags = 0_u32;
    // SAFETY: an open handle; the call only reads its flags.
    unsafe { GetHandleInformation(HANDLE(handle.as_raw_handle()), &raw mut flags) }.unwrap();
    flags & HANDLE_FLAG_INHERIT.0 != 0
}

/// This test binary, started suspended (it never runs) with `listed` passed on, through the
/// same handle list as a `LowBox` launch but this process's own token.
fn spawn_listing(listed: &[RawHandle]) -> Spawned {
    let mut command = LowBoxCommand::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .stdin(Stdio::Null)
        .stdout(Stdio::Null)
        .stderr(Stdio::Null);
    for h in listed {
        command.inherit_handle(*h);
    }
    command.spawn_as(None).unwrap()
}

fn end(child: &Spawned) {
    // SAFETY: the child's process handle has all access.
    unsafe { TerminateProcess(HANDLE(child.process.as_raw_handle()), 1) }.unwrap();
}

#[test]
fn only_the_listed_handles_reach_the_child() {
    let listed = event(false);
    let stray = event(true);
    let child = spawn_listing(&[listed.as_raw_handle()]);
    let (got_listed, got_stray) = (child_holds(&child, &listed), child_holds(&child, &stray));
    end(&child);
    assert!(got_listed, "the listed handle did not reach the child");
    assert!(
        !got_stray,
        "an inheritable handle outside the list reached the child"
    );
    assert!(
        !inheritable(&listed),
        "the listed handle stayed inheritable after the spawn"
    );
    assert!(inheritable(&stray), "the stray handle's own flag changed");
}

#[test]
fn a_repeated_or_null_handle_is_refused() {
    let listed = event(false);
    let mut command = LowBoxCommand::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .inherit_handle(listed.as_raw_handle())
        .inherit_handle(listed.as_raw_handle());
    assert!(matches!(
        command.spawn_as(None),
        Err(Error::InheritList { .. })
    ));
    let mut command = LowBoxCommand::new(std::env::current_exe().unwrap());
    command.env_clear().inherit_handle(std::ptr::null_mut());
    assert!(matches!(
        command.spawn_as(None),
        Err(Error::InheritList { .. })
    ));
    assert!(!inheritable(&listed));
}
