//! The pipe transport on Windows, as the CI user on both ends (the guard's end and the bridge's
//! are the same account here, which the checks allow when the SIDs say so): a round trip with a
//! read and a write in flight at once, the client SID check, a name someone holds already, the
//! bridge's owner check, the pipe's flags, its DACL and its Low label, a shutdown that wakes a
//! blocked read, and read and write timeouts.
#![expect(
    unsafe_code,
    reason = "FFI to read the pipe's flags and descriptor back, and to squat a name"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
    LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeInfo, NAMED_PIPE_MODE, PIPE_SERVER_END, PIPE_TYPE_MESSAGE,
};
use windows::core::PCWSTR;

use super::sid::{self, Sid};
use super::{CLIENT_RIGHTS, MAX_INSTANCES, MakeError, Pipe, PipeListener, connect_within};
use crate::transport::{Connection, Listener, Peer};

/// SYSTEM: a SID that is never the CI user.
const SYSTEM: &str = "S-1-5-18";
/// The Low mandatory level.
const LOW: &str = "S-1-16-4096";
/// `SYSTEM_MANDATORY_LABEL_ACE_TYPE` and `SYSTEM_MANDATORY_LABEL_NO_WRITE_UP`.
const LABEL_ACE: u8 = 0x11;
const NO_WRITE_UP: u32 = 0x1;
/// `ACCESS_ALLOWED_ACE_TYPE`, `FILE_ALL_ACCESS` (what `GA` maps to on a file object) and
/// `FILE_CREATE_PIPE_INSTANCE`.
const ALLOWED_ACE: u8 = 0x0;
const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
const CREATE_PIPE_INSTANCE: u32 = 0x4;
const WAIT: Duration = Duration::from_secs(3);

/// A pipe name no other test or run uses.
fn name(test: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    format!("clauseo-test-{test}-{}-{n}", std::process::id())
}

fn own() -> String {
    sid::current_user().unwrap().text().unwrap()
}

fn read_line(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line
}

#[test]
fn the_right_user_connects_and_lines_go_both_ways_while_a_read_waits() {
    let name = name("round");
    let listener = PipeListener::make(&name, &own()).unwrap();
    let guard = thread::spawn(move || {
        let pipe = listener.accept().unwrap();
        assert!(
            pipe.peer_is(Peer::SameUser),
            "the client SID check refused the CI user"
        );
        let mut reader = BufReader::new(pipe.try_clone().unwrap());
        let line = read_line(&mut reader);
        let mut writer = pipe.try_clone().unwrap();
        writer.write_all(format!("echo {line}").as_bytes()).unwrap();
        // The bridge's close ends the guard's input.
        assert_eq!(read_line(&mut reader), "");
    });
    let server = Sid::parse(&own()).unwrap();
    let client = connect_within(&name, &server, WAIT).unwrap();
    assert!(
        client.peer_is(Peer::SameUser),
        "the owner check refused the guard's pipe"
    );
    // A read in flight first: a write on another handle must not wait for it.
    let mut reader = BufReader::new(client.try_clone().unwrap());
    let reading = thread::spawn(move || read_line(&mut reader));
    let mut writer = client.try_clone().unwrap();
    writer.write_all(b"hello\n").unwrap();
    assert_eq!(reading.join().unwrap(), "echo hello\n");
    drop((writer, client));
    guard.join().unwrap();
}

#[test]
fn a_client_of_another_user_fails_the_check_and_is_disconnected() {
    let name = name("other");
    // The CI user can open it as the pipe's owner; the check wants SYSTEM.
    let listener = PipeListener::make(&name, SYSTEM).unwrap();
    let guard = thread::spawn(move || {
        let pipe = listener.accept().unwrap();
        let checked = pipe.peer_is(Peer::SameUser);
        // What the guard's accept loop does with a refused peer: drop it.
        drop(pipe);
        (checked, listener)
    });
    let client = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let (checked, listener) = guard.join().unwrap();
    assert!(!checked, "a client that is not the client SID passed");
    let mut reader = BufReader::new(client);
    assert_eq!(
        read_line(&mut reader),
        "",
        "the refused client stayed connected"
    );
    drop(listener);
}

#[test]
fn a_name_that_exists_is_taken() {
    let name = name("squat");
    let wide = sid::wide(&format!(r"\\.\pipe\{name}"));
    // SAFETY: `wide` is NUL-terminated; default security, as a squatter's would be; the handle
    // returned is owned below.
    let raw = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            NAMED_PIPE_MODE(0),
            1,
            0,
            0,
            0,
            None,
        )
    };
    assert!(!raw.is_invalid());
    // SAFETY: the handle just created, owned once.
    let squatter = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    assert!(matches!(
        PipeListener::make(&name, &own()),
        Err(MakeError::Taken)
    ));
    drop(squatter);
    // A guard's own pipe is taken too, for the next guard.
    let name = self::name("again");
    let first = PipeListener::make(&name, &own()).unwrap();
    assert!(matches!(
        PipeListener::make(&name, &own()),
        Err(MakeError::Taken)
    ));
    drop(first);
    assert!(matches!(
        PipeListener::make("a", "S-1-5-18-not"),
        Err(MakeError::BadSid)
    ));
}

#[test]
fn the_bridge_refuses_a_pipe_another_user_owns() {
    let name = name("owner");
    let listener = PipeListener::make(&name, &own()).unwrap();
    // The bridge's peer check (`browser_bridge::serve` drops a connection that fails it).
    let other = connect_within(&name, &Sid::parse(SYSTEM).unwrap(), WAIT).unwrap();
    assert!(
        !other.peer_is(Peer::SameUser),
        "the bridge took a pipe SYSTEM does not own"
    );
    drop(other);
    let guards = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    assert!(
        guards.peer_is(Peer::SameUser),
        "the bridge refused the guard's own pipe"
    );
    drop(guards);
    assert!(connect_within(&self::name("missing"), &Sid::parse(&own()).unwrap(), WAIT).is_err());
    drop(listener);
}

/// The pipe's flags as `GetNamedPipeInfo` reports them. It reports the type, the end and the
/// instance limit, not `PIPE_REJECT_REMOTE_CLIENTS`, which no call reads back.
#[test]
fn the_pipe_is_a_byte_pipe_with_few_instances() {
    let name = name("flags");
    let listener = PipeListener::make(&name, &own()).unwrap();
    let waiting = listener.lock();
    let server = waiting.as_ref().unwrap();
    let (mut flags, mut instances) = (NAMED_PIPE_MODE(0), 0u32);
    // SAFETY: the open server handle and two live outputs.
    unsafe {
        GetNamedPipeInfo(
            raw(server),
            Some(&raw mut flags),
            None,
            None,
            Some(&raw mut instances),
        )
    }
    .unwrap();
    assert_eq!(flags.0 & PIPE_SERVER_END.0, PIPE_SERVER_END.0);
    assert_eq!(flags.0 & PIPE_TYPE_MESSAGE.0, 0, "a message pipe");
    assert_eq!(instances, MAX_INSTANCES);
}

/// One ACE: its type, mask and SID.
#[derive(Debug, PartialEq, Eq)]
struct Ace {
    kind: u8,
    mask: u32,
    sid: String,
}

/// The ACEs of `acl`. A label ACE has the allowed ACE's layout.
fn aces(acl: *const ACL) -> Vec<Ace> {
    // SAFETY: `acl` points into a descriptor the caller keeps alive.
    let count = unsafe { (*acl).AceCount };
    (0..u32::from(count))
        .map(|index| {
            let mut ace = std::ptr::null_mut();
            // SAFETY: `index` is below the ACL's count; `ace` is live to fill.
            unsafe { GetAce(acl, index, &raw mut ace) }.unwrap();
            // SAFETY: GetAce points to an ACE inside the ACL; both kinds read here share the
            // ACCESS_ALLOWED_ACE layout (header, mask, SID).
            let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            let header: ACE_HEADER = allowed.Header;
            let sid = PSID((&raw const allowed.SidStart).cast_mut().cast());
            Ace {
                kind: header.AceType,
                mask: allowed.Mask,
                sid: text_of(sid),
            }
        })
        .collect()
}

/// The string of the SID at `sid`.
fn text_of(sid: PSID) -> String {
    let mut text = windows::core::PWSTR::null();
    // SAFETY: `sid` is a valid SID inside a live descriptor; `text` is live to fill.
    unsafe { windows::Win32::Security::Authorization::ConvertSidToStringSidW(sid, &raw mut text) }
        .unwrap();
    // SAFETY: a NUL-terminated string Windows allocated, read and then freed once.
    let string = String::from_utf16(unsafe { text.as_wide() }).unwrap();
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc; freed once.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    string
}

#[test]
fn the_dacl_names_two_users_and_the_label_is_low_no_write_up() {
    let name = name("label");
    let listener = PipeListener::make(&name, SYSTEM).unwrap();
    // The bridge's own handle: what it may read is what the client sees.
    let client = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let (mut owner, mut dacl, mut sacl) =
        (PSID::default(), std::ptr::null_mut(), std::ptr::null_mut());
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the client's open handle (it has READ_CONTROL) and live outputs; the descriptor
    // is freed below, after the last read through the pointers into it.
    let status = unsafe {
        GetSecurityInfo(
            raw(&client),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            Some(&raw mut dacl),
            Some(&raw mut sacl),
            Some(&raw mut descriptor),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let own = own();
    assert_eq!(text_of(owner), own);
    let dacl = aces(dacl);
    let sacl = aces(sacl);
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc; nothing reads it any more.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
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
                mask: FILE_ALL_ACCESS,
                sid: own
            },
        ]
    );
    assert_eq!(
        CLIENT_RIGHTS & CREATE_PIPE_INSTANCE,
        0,
        "the client may add instances"
    );
    assert_eq!(
        sacl,
        [Ace {
            kind: LABEL_ACE,
            mask: NO_WRITE_UP,
            sid: LOW.to_owned()
        }]
    );
    drop((client, listener));
}

#[test]
fn reads_and_writes_time_out_and_a_shutdown_ends_a_blocked_read() {
    let name = name("shut");
    let listener = PipeListener::make(&name, &own()).unwrap();
    let guard = thread::spawn(move || (listener.accept().unwrap(), listener));
    let client = connect_within(&name, &Sid::parse(&own()).unwrap(), WAIT).unwrap();
    let (server, listener) = guard.join().unwrap();
    // Nothing to read: a read timeout passes.
    client
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let timed_out = client
        .try_clone()
        .unwrap()
        .read(&mut [0u8; 16])
        .unwrap_err();
    assert_eq!(
        timed_out.kind(),
        std::io::ErrorKind::TimedOut,
        "{timed_out}"
    );
    client.set_read_timeout(None).unwrap();
    let mut reader = client.try_clone().unwrap();
    let reading = thread::spawn(move || reader.read(&mut [0u8; 16]).unwrap());
    client.shutdown(Shutdown::Both).unwrap();
    assert_eq!(reading.join().unwrap(), 0, "the blocked read did not end");
    // The client never reads: a write past the buffer waits, then times out.
    server
        .set_write_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut writer = server.try_clone().unwrap();
    let error = writer.write_all(&vec![b'x'; 4 << 20]).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut, "{error}");
    drop((client, server, listener));
}

fn raw(pipe: &Pipe) -> HANDLE {
    HANDLE(pipe.handle().as_raw_handle())
}
