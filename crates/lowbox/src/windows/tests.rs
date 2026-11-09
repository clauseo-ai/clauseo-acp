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
use ::windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
use ::windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
};

use ::windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use ::windows::Win32::Security::FreeSid;
use ::windows::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use ::windows::core::{HSTRING, PWSTR};

use crate::{
    Error, HandleProblem, LowBoxCommand, NamespaceHandles, PackageName, Sid, Slot, Spawned, Stdio,
    UNRESUMED_EXIT, package_sid,
};

/// The derivation matches `DeriveAppContainerSidFromAppContainerName`, which needs no profile,
/// from one SHA-256 block to the three a 64-character name takes.
#[test]
fn the_package_sid_matches_the_api() {
    let names = [
        "Clauseo.Browser.B01",
        "Clauseo.Browser.B02",
        "x",
        "A-b.C",
        // 28 characters: 56 bytes of UTF-16, past one block once padded.
        "Clauseo.Browser.Matter-00001",
        crate::package::tests::LONGEST,
    ];
    for name in names {
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

/// This process's user, as the namespace's builder.
fn me() -> Sid {
    super::token::current_user().unwrap()
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
    let error = NamespaceHandles::from_inherited(&b01(), &me(), &values).unwrap_err();
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
    let error = NamespaceHandles::from_inherited(&b01(), &me(), &values).unwrap_err();
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
        .inherit_handle(listed.as_raw_handle())
        .inherit_handle(listed.as_raw_handle());
    assert!(matches!(
        command.spawn_as(None),
        Err(Error::InheritList { .. })
    ));
    let mut command = LowBoxCommand::new(std::env::current_exe().unwrap());
    command.inherit_handle(std::ptr::null_mut());
    assert!(matches!(
        command.spawn_as(None),
        Err(Error::InheritList { .. })
    ));
    assert!(!inheritable(&listed));
}

#[test]
fn the_environment_is_only_what_the_command_sets() {
    // The test process has PATH and SystemRoot; neither may reach a child that did not set it.
    assert!(std::env::var_os("PATH").is_some());
    let empty = LowBoxCommand::new("x.exe");
    assert_eq!(
        empty.environment().unwrap(),
        [0, 0],
        "an empty env must be an empty block"
    );
    let mut command = LowBoxCommand::new("x.exe");
    command
        .env("TEMP", r"C:\t")
        .env("SystemRoot", r"C:\Windows")
        .env("temp", r"C:\u");
    let block = String::from_utf16(&command.environment().unwrap()).unwrap();
    assert_eq!(block, "SystemRoot=C:\\Windows\0temp=C:\\u\0\0");
    assert!(!block.to_ascii_uppercase().contains("PATH="));
}

/// A value equal to this process's standard output is refused before anything else is asked
/// of it, though it is an open handle.
#[test]
fn a_standard_handle_is_never_taken_over() {
    // SAFETY: GetStdHandle only reads this process's parameters.
    let Ok(out) = (unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }) else {
        return;
    };
    if out.is_invalid() || out.0.is_null() {
        // No standard output to take: nothing to check.
        return;
    }
    let events: Vec<OwnedHandle> = (0..5).map(|_| event(false)).collect();
    let mut values = vec![out.0.expose_provenance()];
    values.extend(events.iter().map(|e| e.as_raw_handle().expose_provenance()));
    let error = NamespaceHandles::from_inherited(&b01(), &me(), &values).unwrap_err();
    assert_eq!(
        error,
        Error::Handle {
            slot: Slot::Directory,
            problem: HandleProblem::StandardHandle
        }
    );
}

/// The owner side's inheritable guard turns the flag on for every handle and off again when
/// dropped (six events stand in for the namespace's objects).
#[test]
fn the_inheritable_guard_resets_every_flag() {
    let events: Vec<OwnedHandle> = (0..6).map(|_| event(false)).collect();
    let raw: Vec<RawHandle> = events.iter().map(AsRawHandle::as_raw_handle).collect();
    let copies: Vec<OwnedHandle> = raw
        .iter()
        .map(|h| {
            let mut copy = HANDLE::default();
            // SAFETY: `h` is open in this process; the copy is owned below.
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    HANDLE(*h),
                    GetCurrentProcess(),
                    &raw mut copy,
                    0,
                    false,
                    DUPLICATE_SAME_ACCESS,
                )
            }
            .unwrap();
            // SAFETY: DuplicateHandle succeeded, so `copy` is open and ours alone.
            unsafe { OwnedHandle::from_raw_handle(copy.0) }
        })
        .collect();
    let handles = super::namespace::NamespaceHandles::new(package_sid(&b01()), copies).unwrap();
    let lock = clauseo_process::spawn_lock();
    let guard = handles.inheritable(&lock).unwrap();
    let flags = |h: &NamespaceHandles| -> Vec<bool> {
        h.handles()
            .iter()
            .map(|raw| {
                let mut flags = 0_u32;
                // SAFETY: an open handle of `h`; the call only reads its flags.
                unsafe { GetHandleInformation(*raw, &raw mut flags) }.unwrap();
                flags & HANDLE_FLAG_INHERIT.0 != 0
            })
            .collect()
    };
    assert_eq!(flags(&handles), [true; 6]);
    drop(guard);
    assert_eq!(flags(&handles), [false; 6]);
    drop(lock);
}

/// The drop seam: the end runs once for a child never resumed, and not after a resume.
#[test]
fn the_drop_ends_only_an_unresumed_child() {
    let mut child = spawn_listing(&[]);
    let mut ends = 0;
    child.end_if_unresumed(|_| ends += 1);
    assert_eq!(ends, 1, "a suspended child must be ended");
    end(&child);
    // SAFETY: the child's process handle has SYNCHRONIZE.
    unsafe { WaitForSingleObject(HANDLE(child.process.as_raw_handle()), 10_000) };
    // A resume of an ended process's thread still succeeds; what matters is the flag after.
    let resumed = child.resume();
    let mut after = 0;
    child.end_if_unresumed(|_| after += 1);
    if resumed.is_ok() {
        assert_eq!(after, 0, "a resumed child must not be ended");
    } else {
        assert_eq!(after, 1, "a failed resume must leave the child to be ended");
    }
}

/// Dropped before resume, the real child is ended with `UNRESUMED_EXIT`.
#[test]
fn a_dropped_suspended_child_is_terminated() {
    let child = spawn_listing(&[]);
    let mut process = HANDLE::default();
    // SAFETY: the child's process handle is open; the copy is owned below and outlives the drop.
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            HANDLE(child.process.as_raw_handle()),
            GetCurrentProcess(),
            &raw mut process,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .unwrap();
    // SAFETY: DuplicateHandle succeeded, so `process` is open and ours alone.
    let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
    drop(child);
    // SAFETY: an open process handle with SYNCHRONIZE.
    let waited = unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), 10_000) };
    assert_eq!(waited.0, 0, "the dropped child did not exit");
    let mut code = 0_u32;
    // SAFETY: an open process handle with PROCESS_QUERY_INFORMATION; `code` gets its code.
    unsafe { GetExitCodeProcess(HANDLE(process.as_raw_handle()), &raw mut code) }.unwrap();
    assert_eq!(code, UNRESUMED_EXIT);
}
