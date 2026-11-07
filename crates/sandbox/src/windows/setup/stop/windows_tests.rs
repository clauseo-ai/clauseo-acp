//! The stop's process reads on a real machine (`windows_stopper.rs`): a process whose token's
//! DACL shuts everyone out, SYSTEM included, is still read as its user's from the kernel's
//! process list, its owner is still read, and the stop's selection picks it.
//!
//! The process is a benign one of the test's own (`ping.exe` against the loopback address), run
//! as this test's user, not a pool user: making a pool account and starting a process as it
//! (`CreateProcessWithLogonW`) would add an account, a profile and the secondary logon service
//! to the test, while the reads under test are the same for any user. Elevated, as CI's
//! elevated step runs it (`CLAUSEO_ELEVATED_TESTS=1`): the list gives every process's SID to an
//! administrator only.
#![expect(unsafe_code, reason = "FFI to the child's token")]

use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SetKernelObjectSecurity, TOKEN_WRITE_DAC,
};
use windows::Win32::Storage::FileSystem::READ_CONTROL;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::pick;
use super::windows_stopper::{look_at, processes};
use crate::windows::acl::object::{Handle, words};
use crate::windows::acl_bytes::{build_descriptor, empty_acl};
use crate::windows::record::current_user_sid;
use crate::windows::setup::processes::owner_of;
use crate::windows::test_dacl::elevated;

/// Whether the test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// The child, ended and waited for on drop.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// Gives the child's token an empty DACL: no one may query it, SYSTEM included. This process
/// opens it with `WRITE_DAC`, which the token's default DACL grants Administrators.
fn shut_token(child: &Child) {
    let process = HANDLE(child.as_raw_handle());
    let mut token = HANDLE::default();
    // SAFETY: `process` is the child's live handle, kept by `child`; `token` is a live
    // out-pointer, owned by `Handle` once written.
    unsafe { OpenProcessToken(process, TOKEN_WRITE_DAC, &raw mut token) }.unwrap();
    let token = Handle(token);
    let mut descriptor = words(&build_descriptor(0, Some(&empty_acl()), None));
    // SAFETY: the token is open with WRITE_DAC; `descriptor` is a valid self-relative
    // descriptor that lives for the call.
    unsafe {
        SetKernelObjectSecurity(
            token.0,
            DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast()),
        )
    }
    .unwrap();
}

/// This process's own owner, as the child's should be: both come from this token's default.
fn own_owner() -> Option<crate::windows::sids::Sid> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_ACCESS_RIGHTS(READ_CONTROL.0);
    // SAFETY: plain values; the handle is owned by `Handle` from here.
    let handle = unsafe { OpenProcess(access, false, std::process::id()) }.unwrap();
    owner_of(&Handle(handle))
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "starts the benign process whose token the test shuts"
)]
fn elevated_a_process_whose_token_shuts_out_system_is_still_read_and_picked() {
    if !gate("elevated_a_process_whose_token_shuts_out_system_is_still_read_and_picked") {
        return;
    }
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let child = Command::new(format!(r"{root}\System32\ping.exe"))
        .args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child = Running(child);
    let pid = child.0.id();
    shut_token(&child.0);
    let me = current_user_sid().unwrap();

    // Through its own handle: the token can't be read; the owner can, and stands in for it.
    let (process, handle) = look_at(pid).unwrap();
    assert!(handle.is_some() && process.can_stop, "{process:?}");
    assert_eq!(process.token, None, "its token's DACL lets no one query it");
    assert!(process.owner.is_some(), "{process:?}");
    assert_eq!(process.owner, own_owner());

    // The kernel's list names its user all the same, and the selection picks it.
    let (seen, handles) = processes().unwrap();
    let listed = seen.iter().find(|process| process.pid == pid).unwrap();
    assert_eq!(listed.listed.as_ref(), Some(&me), "{listed:?}");
    assert_eq!(listed.token, None);
    assert!(handles.iter().any(|(at, _)| *at == pid));
    let picked = pick(&seen, std::slice::from_ref(&me), std::process::id());
    assert!(picked.stop.contains(&(pid, me)), "{picked:?}");
    drop(child);
}
