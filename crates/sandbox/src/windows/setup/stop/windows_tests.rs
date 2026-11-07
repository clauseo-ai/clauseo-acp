//! The stop's process reads on a real machine (`windows_stopper.rs`): a process whose token's
//! DACL shuts everyone out, SYSTEM included, is left unnamed by the kernel's process list
//! (`WTSEnumerateProcessesExW` gives it no SID, CI run 42416074600), but its owner is still read
//! under `SeDebugPrivilege`, enabled as `--stop` enables it, and names it: the selection picks
//! it when the owner is a pool SID and counts it unknown (48) otherwise.
//!
//! The process is a benign one of the test's own (`ping.exe` against the loopback address), run
//! as this test's user, not a pool user: making a pool account and starting a process as it
//! (`CreateProcessWithLogonW`) would add an account, a profile and the secondary logon service
//! to the test, while the reads under test are the same for any user: the pool list is this
//! test's choice of SIDs. Elevated, as CI's elevated step runs it (`CLAUSEO_ELEVATED_TESTS=1`):
//! `SeDebugPrivilege` is an administrator's.
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
use crate::windows::setup::processes::{DebugPrivilege, owner_of};
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
fn elevated_a_process_whose_token_shuts_out_system_is_named_by_its_owner_and_picked() {
    if !gate("elevated_a_process_whose_token_shuts_out_system_is_named_by_its_owner_and_picked") {
        return;
    }
    // As `WindowsStopper::new` does for the whole of `--stop`.
    let debug = DebugPrivilege::enable();
    assert!(debug.is_some(), "SeDebugPrivilege couldn't be enabled");
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
    // Through its own handle: the token can't be read. The owner can: `look_at` opens the
    // process with `READ_CONTROL` under `SeDebugPrivilege`, which passes the process's DACL,
    // not the token's, so a process that shuts SYSTEM out of its token can't hide its owner.
    // The owner reads back as its creator, this test's user (the failed run showed `-500`).
    let (process, handle) = look_at(pid).unwrap();
    assert!(handle.is_some() && process.can_stop, "{process:?}");
    assert_eq!(process.token, None, "its token's DACL lets no one query it");
    assert_eq!(process.owner, own_owner(), "{process:?}");
    let owner = process.owner.unwrap();

    // The kernel's list gives it no SID: the token it would read is shut too.
    let (seen, handles) = processes().unwrap();
    let found = seen.iter().find(|process| process.pid == pid).unwrap();
    assert_eq!(found.listed, None, "{found:?}");
    assert_eq!(found.token, None);
    assert_eq!(found.owner.as_ref(), Some(&owner), "{found:?}");
    assert!(handles.iter().any(|(at, _)| *at == pid));

    // Its owner a pool SID: picked, as that user's. Any other owner: unknown, never stopped.
    let alone = std::slice::from_ref(found);
    let picked = pick(alone, std::slice::from_ref(&owner), std::process::id());
    assert_eq!(picked.stop, [(pid, owner.clone())], "{picked:?}");
    let other = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1010").unwrap();
    assert_ne!(other, owner);
    let picked = pick(alone, std::slice::from_ref(&other), std::process::id());
    assert!(picked.stop.is_empty() && picked.unknown == 1, "{picked:?}");
    drop(debug);
    drop(child);
}
