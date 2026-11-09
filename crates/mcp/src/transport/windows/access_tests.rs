//! The pipe's access checks on Windows, as the CI user: with a package SID its ACE and the
//! refusal of a client outside any app container; an anonymous client refused; the bridge's check
//! of the server's process identity; and the guard's grant on its own process. A client in a real
//! `LowBox` token (the accept case) is stage C's VM row: CI cannot make one cheaply.
#![expect(
    unsafe_code,
    reason = "FFI to read the pipe's and the process's DACLs back, and to open an anonymous client"
)]

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::thread;

use windows::Win32::Foundation::{ERROR_SUCCESS, GENERIC_READ, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_NONE, FILE_WRITE_DATA, OPEN_EXISTING, SECURITY_ANONYMOUS,
    SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::PCWSTR;

use super::sid::{self, Sid};
use super::tests::{ALLOWED_ACE, Ace, FILE_ALL_ACCESS, SYSTEM, WAIT, aces, name, own, raw};
use super::{CLIENT_RIGHTS, MakeError, Pipe, PipeListener, connect_within};
use crate::transport::{Connection, Identity, Listener, Peer};

/// A package SID of the agent's shape (`S-1-15-2-` and seven numbers); no such package exists.
const PACKAGE: &str = "S-1-15-2-11-12-13-14-15-16-17";

/// The pipe's DACL as `aces` reads it, from the bridge's own handle.
fn dacl_of(pipe: &Pipe) -> Vec<Ace> {
    let (mut dacl, mut descriptor) = (std::ptr::null_mut(), PSECURITY_DESCRIPTOR::default());
    // SAFETY: an open handle with READ_CONTROL and live outputs; the descriptor is freed below,
    // after the last read through `dacl`.
    let status = unsafe {
        GetSecurityInfo(
            raw(pipe),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&raw mut dacl),
            None,
            Some(&raw mut descriptor),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let read = aces(dacl);
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc; nothing reads it any more.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    read
}

#[test]
fn a_package_sid_adds_its_ace_and_never_all_application_packages() {
    let name = name("package");
    let listener = PipeListener::make(&name, SYSTEM, Some(PACKAGE)).unwrap();
    let client = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let dacl = dacl_of(&client);
    let own = own();
    assert_eq!(
        dacl,
        [
            Ace {
                kind: ALLOWED_ACE,
                mask: CLIENT_RIGHTS,
                sid: SYSTEM.to_owned()
            },
            Ace {
                kind: ALLOWED_ACE,
                mask: CLIENT_RIGHTS,
                sid: PACKAGE.to_owned()
            },
            Ace {
                kind: ALLOWED_ACE,
                mask: FILE_ALL_ACCESS,
                sid: own
            },
        ]
    );
    assert!(
        dacl.iter()
            .all(|ace| !ace.sid.starts_with("S-1-15-2-1") || ace.sid == PACKAGE),
        "ALL APPLICATION PACKAGES in {dacl:?}"
    );
    drop((client, listener));
    for bad in ["S-1-15-2-1", "S-1-5-18", "S-1-15-3-1024-1-2-3-4-5-6-7-8"] {
        assert!(
            matches!(
                PipeListener::make(&self::name("bad-package"), &self::own(), Some(bad)),
                Err(MakeError::BadSid)
            ),
            "{bad}"
        );
    }
}

#[test]
fn with_a_package_sid_a_client_outside_any_app_container_is_refused() {
    let name = name("not-contained");
    // The CI user is the client SID; only the package does not match.
    let listener = PipeListener::make(&name, &own(), Some(PACKAGE)).unwrap();
    let guard = thread::spawn(move || {
        let pipe = listener.accept().unwrap();
        let checked = pipe.peer_is(Peer::SameUser);
        drop(pipe);
        (checked, listener)
    });
    let client = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let (checked, listener) = guard.join().unwrap();
    assert!(
        !checked,
        "a client with no app container passed the package check"
    );
    drop((client, listener));
}

/// `SECURITY_ANONYMOUS`: the guard can learn nothing of the client, so the check refuses it.
#[test]
fn an_anonymous_client_is_refused_and_nothing_breaks() {
    let name = name("anonymous");
    let listener = PipeListener::make(&name, &own(), None).unwrap();
    let guard = thread::spawn(move || {
        let pipe = listener.accept().unwrap();
        let checked = pipe.peer_is(Peer::SameUser);
        drop(pipe);
        // The accept loop goes on: a next client is checked as usual.
        let next = listener.accept().unwrap();
        (checked, next.peer_is(Peer::SameUser), listener)
    });
    let wide = sid::wide(&format!(r"\\.\pipe\{name}"));
    // SAFETY: `wide` is NUL-terminated; the handle returned is owned below. SECURITY_ANONYMOUS
    // is 0, so SECURITY_SQOS_PRESENT alone asks for it.
    let raw = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0 | FILE_WRITE_DATA.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
            None,
        )
    }
    .unwrap();
    // SAFETY: the handle just opened, owned once.
    let anonymous = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let next = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let (checked, next_checked, listener) = guard.join().unwrap();
    assert!(!checked, "an anonymous client passed as the client SID");
    assert!(next_checked, "the next client was refused");
    drop((anonymous, next, listener));
}

#[test]
fn the_bridge_checks_the_servers_process_identity() {
    let name = name("identity");
    let listener = PipeListener::make(&name, &own(), None).unwrap();
    let server = Sid::parse(&own()).unwrap();
    // This test process is the guard here.
    let guard = super::own_identity().unwrap();
    let wrong_pid = Identity {
        pid: guard.pid + 4,
        ..guard
    };
    let wrong_time = Identity {
        version: guard.version + 1,
        ..guard
    };
    for (identity, passes) in [(guard, true), (wrong_pid, false), (wrong_time, false)] {
        let client = connect_within(&name, &server, WAIT).unwrap();
        assert_eq!(
            client.peer_is(Peer::Process(identity)),
            passes,
            "{identity}"
        );
        drop(client);
    }
    drop(listener);
}

/// The guard grants the client user and package `PROCESS_QUERY_LIMITED_INFORMATION` (0x1000)
/// on its own process, and nothing more.
#[test]
fn the_guard_lets_the_client_query_its_process_and_nothing_more() {
    let user = "S-1-5-21-1-2-3-4242";
    let listener = PipeListener::make(&name("grant"), user, Some(PACKAGE)).unwrap();
    let (mut dacl, mut descriptor) = (std::ptr::null_mut(), PSECURITY_DESCRIPTOR::default());
    // SAFETY: the pseudo handle of this process (every right) and live outputs; the descriptor
    // is freed below, after the last read through `dacl`.
    let status = unsafe {
        GetSecurityInfo(
            GetCurrentProcess(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&raw mut dacl),
            None,
            Some(&raw mut descriptor),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let read = aces(dacl);
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc; nothing reads it any more.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    for sid in [user, PACKAGE] {
        let granted: Vec<&Ace> = read.iter().filter(|ace| ace.sid == sid).collect();
        assert_eq!(
            granted,
            [&Ace {
                kind: ALLOWED_ACE,
                mask: 0x1000,
                sid: sid.to_owned()
            }],
            "{read:?}"
        );
    }
    drop(listener);
}
