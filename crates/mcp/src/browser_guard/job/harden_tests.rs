//! Windows: a child of a hardened job, and the guard's own process once hardened, deny another
//! process of the user (this test) `PROCESS_VM_READ`, `PROCESS_DUP_HANDLE` and `WRITE_DAC`, and
//! grant it only `PROCESS_QUERY_LIMITED_INFORMATION` and `SYNCHRONIZE` (route 2's row a1h, the
//! proof row R4). Runs in the Windows CI job (`cargo nextest`, one process per test, so hardening
//! this test's own process touches no other test).
#![expect(unsafe_code, reason = "OpenProcess by pid, to probe the hardened DACL")]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_VM_READ,
};

use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::HANDLE;

use super::super::harden::{Hardened, is_hardened};
use super::{Io, Job, Launch};

/// `WRITE_DAC`, as a process access right.
const WRITE_DAC: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0004_0000);

/// Whether this process may open `pid` for `access`.
fn opens(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> bool {
    // SAFETY: plain values; a handle returned is closed at once by its drop below.
    match unsafe { OpenProcess(access, false, pid) } {
        Ok(handle) => {
            // SAFETY: `handle` is the open handle just returned, owned and closed once here.
            drop(unsafe {
                <std::os::windows::io::OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(
                    handle.0,
                )
            });
            true
        }
        Err(_) => false,
    }
}

fn assert_hardened(pid: u32) {
    assert!(!opens(pid, PROCESS_VM_READ), "PROCESS_VM_READ was granted");
    assert!(
        !opens(pid, PROCESS_DUP_HANDLE),
        "PROCESS_DUP_HANDLE was granted"
    );
    assert!(!opens(pid, WRITE_DAC), "WRITE_DAC was granted");
    assert!(
        opens(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE),
        "the probes the host and the guard make were refused"
    );
}

/// A built example, as `tests.rs` finds them.
fn example(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().and_then(Path::parent).unwrap();
    profile.join("examples").join(format!("{name}.exe"))
}

/// A hanging `echo_stub` started in `job`.
fn hanging_child(job: &Job) -> super::Child {
    let dir = std::env::temp_dir();
    let args = [
        dir.join(format!("clauseo-harden-{}.log", std::process::id()))
            .into_os_string(),
        OsString::from("hang"),
    ];
    job.spawn(&Launch {
        program: &example("echo_stub"),
        args: &args,
        cwd: &dir,
        env: &BTreeMap::new(),
        stdin: Io::Piped,
        stdout: Io::Piped,
        stderr: Io::Null,
        inherit: &[],
    })
    .unwrap()
}

/// The check the guard's hardened copy makes of itself (`Hardened::verified`): a process
/// created with the descriptors passes it, an ordinary one does not, so a forged
/// `--hardened-copy` makes a guard that refuses to start.
#[test]
fn the_check_of_the_copy_passes_a_process_created_hardened_and_no_other() {
    for (job, expected) in [
        (Job::new(Hardened::apply().unwrap()).unwrap(), true),
        (Job::unhardened().unwrap(), false),
    ] {
        let mut child = hanging_child(&job);
        let handle = HANDLE(child.as_raw_handle());
        assert_eq!(is_hardened(handle).unwrap(), expected);
        assert!(job.kill());
        drop(child.wait());
    }
}

#[test]
fn a_child_of_a_hardened_job_is_hardened_from_its_start() {
    let job = Job::new(Hardened::apply().unwrap()).unwrap();
    let dir = std::env::temp_dir();
    let args = [
        dir.join(format!("clauseo-harden-{}.log", std::process::id()))
            .into_os_string(),
        OsString::from("hang"),
    ];
    let mut child = job
        .spawn(&Launch {
            program: &example("echo_stub"),
            args: &args,
            cwd: &dir,
            env: &BTreeMap::new(),
            stdin: Io::Piped,
            stdout: Io::Piped,
            stderr: Io::Null,
            inherit: &[],
        })
        .unwrap();
    assert_hardened(child.id());
    assert!(job.kill());
    drop(child.wait());
}

#[test]
fn the_guard_hardens_its_own_process() {
    let hardened = Hardened::apply().unwrap();
    assert_hardened(std::process::id());
    drop(hardened);
}
