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

use super::windows_bits::{self, Com};
use super::windows_stopper::{WindowsStopper, look_at, processes};
use super::{Image, Passes, Stopper as _, cancel_jobs, pick};
use crate::test_support::TempDir;
use crate::windows::acl::object::{Handle, words};
use crate::windows::acl_bytes::{build_descriptor, empty_acl};
use crate::windows::refresh::windows_tests::{system32, try_in_cmd};
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::processes::{DebugPrivilege, owner_of};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::{elevated, protect_with};

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

/// The BITS jobs of `owner`, all users' listed as `windows_bits` lists them for the stop.
fn jobs_of(owner: &Sid) -> usize {
    let jobs = windows_bits::jobs().unwrap();
    jobs.iter()
        .filter(|(_, by)| by.as_ref() == Some(owner))
        .count()
}

/// The test user, with every BITS job it owns cancelled as this administrator before the
/// account and its profile go (`TestUser`'s drop, after this one's).
struct WithJobs(TestUser);

impl Drop for WithJobs {
    fn drop(&mut self) {
        let _com = Com::new();
        match windows_bits::jobs() {
            Ok(jobs) => {
                for (job, owner) in &jobs {
                    if owner.as_ref() == Some(&self.0.sid)
                        && let Err(why) = windows_bits::cancel(job)
                    {
                        eprintln!("a job of {} wasn't cancelled: {why}", self.0.name);
                    }
                }
            }
            Err(why) => eprintln!("the BITS jobs weren't listed for the clean-up: {why}"),
        }
    }
}

/// `--stop`'s BITS step (`cancel_jobs`, over the real `WindowsStopper`) cancels a job a pool
/// user left queued. CI runs it as an elevated administrator, not as LOCAL SYSTEM: the run as
/// SYSTEM, from the task, is a VM row. A cancel that fails, `ERROR_NOT_LOGGED_ON` (1245)
/// included, fails the test: that is the evidence the step works for a logged-off user.
#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_stop_cancels_a_pool_users_bits_job() {
    if !gate("elevated_the_stop_cancels_a_pool_users_bits_job") {
        return;
    }
    // 1. A standard account, as the task test makes one.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = WithJobs(TestUser::named(TASK_TEST_PREFIX, &random_hex(3)));
    let sid = user.0.sid.clone();
    eprintln!("1. the test user: {} {}", user.0.name, sid.as_str());
    let dir = TempDir::new("stop-bits");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();

    // 2. As that user, unelevated: a download job with no file, left queued.
    let job = format!("ClauseoStopProbe{}", std::process::id());
    let create = format!(r#""{}\bitsadmin.exe" /create /download {job}"#, system32());
    let (code, printed) = try_in_cmd(&user.0, dir.path(), &create);
    eprintln!("2. as the test user: {create}\nexit {code:?}\n{printed}");
    assert_eq!(
        code,
        Some(0),
        "bitsadmin /create as the test user:\n{printed}"
    );

    // 3. As this process: the job is there, owned by the test user.
    let _com = Com::new();
    let before = jobs_of(&sid);
    eprintln!(
        "3. jobs owned by {} before the stop: {before}",
        sid.as_str()
    );
    assert!(before >= 1, "no BITS job is owned by the test user");

    // 4. The stop's own BITS step, with the test user as the pool.
    let mut stopper = WindowsStopper::new();
    let mut passes = Passes::default();
    let pool = std::slice::from_ref(&sid);
    let listed = cancel_jobs(&mut stopper, pool, &mut passes);
    eprintln!(
        "4. cancel_jobs: {listed:?}, {} cancelled, {} not, capped {}",
        passes.jobs_cancelled, passes.jobs_failed, passes.jobs_capped
    );
    assert_eq!(listed, Ok(()));
    assert_eq!(
        passes.jobs_failed, 0,
        "a job wasn't cancelled, or its owner wasn't read"
    );
    assert!(!passes.jobs_capped);
    assert_eq!(passes.jobs_cancelled, before);
    assert!(passes.found.contains(&sid), "the owner is marked as found");
    drop(stopper);
    let after = jobs_of(&sid);
    eprintln!("4. jobs owned by {} after the stop: {after}", sid.as_str());
    assert_eq!(after, 0, "a job of the test user is still listed");
    eprintln!("5. the clean-up cancels anything left, then deletes the user");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_this_machines_unnamed_session_0_processes_are_minimal_ones() {
    if !gate("elevated_this_machines_unnamed_session_0_processes_are_minimal_ones") {
        return;
    }
    // The stop reads under SeDebugPrivilege; with nothing of a pool's running, every session-0
    // process no source names and that can't be opened to be terminated must be a minimal one:
    // no image file, not a subsystem process (`Image::Missing`). Anything else would end every
    // run at 48; each offender is listed with its image's file name.
    let mut stopper = WindowsStopper::new();
    let seen = stopper.snapshot().unwrap();
    let unnamed: Vec<_> = seen
        .iter()
        .filter(|p| p.listed.is_none() && p.token.is_none() && p.owner.is_none())
        .filter(|p| !p.can_stop && p.session == Some(0) && ![0, 4].contains(&p.pid))
        .collect();
    for process in &unnamed {
        eprintln!("unnamed in session 0: {} {:?}", process.pid, process.image);
    }
    let offenders: Vec<String> = unnamed
        .iter()
        .filter(|p| p.image != Image::Missing)
        .map(|p| format!("{} {}", p.pid, p.image.file_name()))
        .collect();
    assert!(
        offenders.is_empty(),
        "unnamed, unopenable session-0 processes that aren't minimal: {offenders:?}"
    );
    let picked = pick(&seen, &[], std::process::id());
    assert_eq!(picked.unknown, 0, "{:?}", picked.unknown_images);
}
