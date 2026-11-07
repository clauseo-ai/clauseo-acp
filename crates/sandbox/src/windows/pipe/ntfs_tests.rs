//! The runner pipes on a real machine: the DACL each gets, one instance, the client's pid, data
//! both ways and the end of the stream; elevated, that another pool user's Low token is refused
//! every pipe, the named pool user's Medium token gets past the DACL, and its Low token is
//! refused by the label.
#![expect(
    unsafe_code,
    reason = "FFI to the pipes' security and an event in tests"
)]

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::mem::ManuallyDrop;
use std::os::windows::io::FromRawHandle as _;
use std::ptr::null_mut;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, LABEL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
};
use windows::Win32::System::Threading::CreateEventW;

use super::os::{ServerPipe, open_client};
use super::{PipeSet, Role, pipe_sddl};
use crate::windows::acl::object::Handle;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;
use crate::windows::test_acl::elevated_tests_enabled;
use crate::windows::test_pool::{
    Impersonating, Owned, TestUser, acl_entries, log_on, low_restricted_token,
};

/// `FILE_GENERIC_READ | FILE_GENERIC_WRITE`: each entry's rights (`pipe::PIPE_RIGHTS`).
const READ_WRITE: u32 = 0x0012_019F;
/// `SYSTEM_MANDATORY_LABEL_ACE_TYPE`.
const LABEL: u8 = 0x11;
/// `SYSTEM_MANDATORY_LABEL_NO_WRITE_UP | SYSTEM_MANDATORY_LABEL_NO_READ_UP`.
const NO_WRITE_OR_READ_UP: u32 = 0x3;
/// `ERROR_PIPE_BUSY`: the one instance is taken, so the open got past the DACL.
const PIPE_BUSY: i32 = 231;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// A manual-reset event never set: a `watch` handle that never fires.
fn never() -> Owned {
    // SAFETY: no attributes and no name; the event is closed by `Owned`.
    Owned(unsafe { CreateEventW(None, true, false, None) }.unwrap())
}

/// The client end `handle` as a `File`, which closes it.
fn file(handle: Handle) -> File {
    let raw = ManuallyDrop::new(handle).0;
    // SAFETY: `raw` is open and owned by the `Handle` alone, whose drop is skipped; the `File`
    // closes it once.
    unsafe { File::from_raw_handle(raw.0) }
}

/// An ACL's entries, as (type, flags, mask, SID).
type Entries = Vec<(u8, u8, u32, Sid)>;

/// What the pipe `handle` names holds: its DACL's entries, whether the DACL is protected, and
/// its label's entries.
fn security_of(handle: &Handle) -> (Entries, bool, Entries) {
    let mut dacl: *mut ACL = null_mut();
    let mut sacl: *mut ACL = null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open with READ_CONTROL (GENERIC_READ); the out-pointers are live;
    // `dacl` and `sacl` point into `descriptor`, freed below after they are read.
    let status = unsafe {
        GetSecurityInfo(
            handle.0,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            None,
            None,
            Some(&raw mut dacl),
            Some(&raw mut sacl),
            Some(&raw mut descriptor),
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    // SAFETY: `dacl` and `sacl` are valid inside the live descriptor.
    let (entries, label) = unsafe { (acl_entries(dacl), acl_entries(sacl)) };
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: the descriptor is live; the out-pointers are live.
    unsafe { GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision) }
        .unwrap();
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    (entries, control & SE_DACL_PROTECTED.0 != 0, label)
}

/// Opens `role`'s client end in `set`: `Ok` when past the DACL (opened, or the one instance
/// busy), else the Win32 error code.
fn past_dacl(set: &PipeSet, role: Role) -> Result<(), i32> {
    match open_client(&set.name(role), role, false) {
        Ok(_) => Ok(()),
        Err(error) => match error.raw_os_error() {
            Some(PIPE_BUSY) => Ok(()),
            code => Err(code.unwrap_or(-1)),
        },
    }
}

/// The four pipes of a new set, with `sddl`.
fn make_all(sddl: &str) -> (PipeSet, Vec<ServerPipe>) {
    let set = PipeSet::random().unwrap();
    let pipes = Role::ALL
        .iter()
        .map(|role| ServerPipe::create(&set.name(*role), *role, sddl).unwrap())
        .collect();
    (set, pipes)
}

#[test]
fn a_pipe_has_its_dacl_one_instance_and_carries_data_both_ways() {
    let own = current_user_sid().unwrap();
    let desktop = sid("S-1-5-21-1-2-3-1001");
    let sddl = pipe_sddl(&desktop, &own);
    let set = PipeSet::random().unwrap();
    let watch = never();
    let wait = Duration::from_secs(10);

    let ctl = ServerPipe::create(&set.name(Role::Ctl), Role::Ctl, &sddl).unwrap();
    assert!(
        ServerPipe::create(&set.name(Role::Ctl), Role::Ctl, &sddl).is_err(),
        "a second instance of the name is refused"
    );
    let client = open_client(&set.name(Role::Ctl), Role::Ctl, false).unwrap();
    ctl.connect(watch.0, wait).unwrap();
    assert_eq!(ctl.client_pid().unwrap(), std::process::id());
    let (entries, protected, label) = security_of(&client);
    assert!(protected);
    assert_eq!(
        entries,
        [(0, 0, READ_WRITE, desktop), (0, 0, READ_WRITE, own)]
    );
    assert_eq!(
        label,
        [(LABEL, 0, NO_WRITE_OR_READ_UP, sid("S-1-16-8192"))],
        "Medium, no write up, no read up"
    );
    let mut client = file(client);
    client.write_all(b"launch").unwrap();
    let mut got = [0u8; 6];
    (&ctl).read_exact(&mut got).unwrap();
    assert_eq!(&got, b"launch");
    (&ctl).write_all(b"report").unwrap();
    client.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"report");

    let out = ServerPipe::create(&set.name(Role::Out), Role::Out, &sddl).unwrap();
    let mut writer = file(open_client(&set.name(Role::Out), Role::Out, false).unwrap());
    out.connect(watch.0, wait).unwrap();
    writer.write_all(b"stdout").unwrap();
    (&out).read_exact(&mut got).unwrap();
    assert_eq!(&got, b"stdout");
    drop(writer);
    assert_eq!(
        (&out).read(&mut got).unwrap(),
        0,
        "the client gone reads as the end"
    );

    let input = ServerPipe::create(&set.name(Role::In), Role::In, &sddl).unwrap();
    let mut reader = file(open_client(&set.name(Role::In), Role::In, false).unwrap());
    input.connect(watch.0, wait).unwrap();
    (&input).write_all(b"stdin!").unwrap();
    reader.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"stdin!");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_another_pool_users_low_token_is_refused_every_pipe() {
    if !elevated_tests_enabled() {
        return;
    }
    let own = current_user_sid().unwrap();
    let user = TestUser::new();

    // Pipes for another pool user: this one's Low token is refused each, by the DACL.
    let (set, _pipes) = make_all(&pipe_sddl(&own, &sid("S-1-5-21-1-2-3-1101")));
    let low = low_restricted_token(&user);
    let refused: Vec<Result<(), i32>> = {
        let _as_pool = Impersonating::start(&low);
        Role::ALL
            .iter()
            .map(|role| past_dacl(&set, *role))
            .collect()
    };
    assert_eq!(refused, [Err(5); 4], "must refuse: another matter's agent");

    // Pipes naming the user: its own token gets past the DACL on each.
    let (set, _pipes) = make_all(&pipe_sddl(&own, &user.sid));
    let logon = log_on(&user);
    let passed: Vec<Result<(), i32>> = {
        let _as_pool = Impersonating::start(&logon);
        Role::ALL
            .iter()
            .map(|role| past_dacl(&set, *role))
            .collect()
    };
    assert_eq!(passed, [Ok(()); 4]);

    // Its own Low token: refused every pipe, the read end too, by the Medium label's
    // no-write-up and no-read-up.
    let (set, _pipes) = make_all(&pipe_sddl(&own, &user.sid));
    let low_refused: Vec<Result<(), i32>> = {
        let _as_pool = Impersonating::start(&low);
        Role::ALL
            .iter()
            .map(|role| past_dacl(&set, *role))
            .collect()
    };
    assert_eq!(
        low_refused,
        [Err(5); 4],
        "must refuse: a Low token opens no pipe by name"
    );
}
