//! Windows: a child of a hardened job, and the guard's own process once hardened, deny the probes
//! of route 2's threat `PROCESS_VM_READ`, `PROCESS_DUP_HANDLE` and `WRITE_DAC`, and grant them
//! only `PROCESS_QUERY_LIMITED_INFORMATION` and `SYNCHRONIZE` (route 2's row a1h, the proof row
//! R4). Each target is probed twice, by two tokens:
//! - a restricted token, as Codex's sandbox runs the agent's shell under: every privilege but
//!   `SeChangeNotify` deleted, BUILTIN\Administrators deny-only, the user's own SID. The probe is
//!   this test binary run again under it ([`restricted_probe_child`], `harden_tests/probe.rs`);
//! - this test process, the same user, with `SeDebugPrivilege` removed from its token first.
//!
//! Not checked, by design: an elevated token with `SeDebugPrivilege` enabled opens any process
//! whatever its DACL. CI's Windows gate is such a chain: an elevated scheduled task runs Git
//! Bash, whose MSYS runtime enables `SeDebugPrivilege`, and nextest under it inherits that; so the
//! same-user probe removes the privilege before it probes, and asserts it is gone. A test that
//! runs elevated says so in its output.
//!
//! A failed check prints the target's security descriptor as SDDL and the probing token (user,
//! owner, integrity, privileges, BUILTIN\Administrators), which tells a DACL not applied from a
//! token that bypasses it. Runs in the Windows CI job (`cargo nextest`, one process per test, so
//! hardening this test's own process or changing its token touches no other test).
#![expect(
    unsafe_code,
    reason = "the current process's pseudo handle, a hardened target"
)]

mod probe;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::GetCurrentProcess;

use super::super::harden::{Hardened, is_hardened};
use super::{Io, Job, Launch};
use probe::Access;

/// Fails unless `access` is the hardened one; `target` is a handle with `READ_CONTROL` to the
/// probed process, `prober` the probing token as [`probe::token_report`] tells it.
fn assert_denied(access: &Access, target: HANDLE, prober: &str) {
    let diagnosis = || {
        format!(
            "\n  target's security descriptor: {}\n  probing token: {prober}",
            probe::sddl_of(target)
        )
    };
    assert!(
        !access.vm_read(),
        "PROCESS_VM_READ was granted{}",
        diagnosis()
    );
    assert!(
        !access.dup_handle(),
        "PROCESS_DUP_HANDLE was granted{}",
        diagnosis()
    );
    assert!(!access.write_dac(), "WRITE_DAC was granted{}", diagnosis());
    assert!(
        access.query(),
        "the probes the host and the guard make were refused{}",
        diagnosis()
    );
}

/// Says in the test's output when this process runs elevated.
fn note_elevation() {
    if probe::elevated() {
        println!(
            "this test runs elevated; its token: {}",
            probe::token_report()
        );
    }
}

/// Probes `pid` from this process once `SeDebugPrivilege` is out of its token, and fails unless
/// it is denied. `target` is a handle to the same process with `READ_CONTROL`.
fn same_user_probe(pid: u32, target: HANDLE) {
    note_elevation();
    let before = probe::token_report();
    probe::remove_debug().unwrap();
    let after = probe::token_report();
    assert!(
        !probe::debug_enabled().unwrap(),
        "SeDebugPrivilege is still enabled after its removal: {after}"
    );
    let prober = format!("{after}\n  (before SeDebugPrivilege was removed: {before})");
    assert_denied(&Access::of(pid), target, &prober);
}

/// Probes `pid` from this test binary run under a restricted token ([`restricted_probe_child`]),
/// and fails unless that token is restricted as Codex's sandbox's is and it is denied. `target`
/// is a handle to the same process with `READ_CONTROL`.
fn restricted_probe(pid: u32, target: HANDLE) {
    note_elevation();
    let path = module_path!();
    let module = path.split_once("::").map_or(path, |split| split.1);
    let entry = format!("{module}::restricted_probe_child");
    let printed = probe::restricted(&entry, pid).unwrap();
    let prober = format!(
        "the restricted probe's, printed in its output below\n  (the test's own: {})\n{printed}",
        probe::token_report()
    );
    let access = Access::parse(&printed)
        .unwrap_or_else(|| panic!("the restricted probe printed no result: {prober}"));
    assert!(
        printed.contains("sedebug=off")
            && printed.contains("administrators=")
            && !printed.contains("administrators=enabled"),
        "the probe's token is not restricted as Codex's sandbox's is: {prober}"
    );
    assert_denied(&access, target, &prober);
}

/// Not a check of its own: the restricted-token probe ([`restricted_probe`]), which runs this
/// test binary again under that token with [`probe::PROBE_PID`] set. It prints which rights it
/// was granted on that pid and its own token. Run any other way, it does nothing.
#[test]
fn restricted_probe_child() {
    let Some(pid) = std::env::var(probe::PROBE_PID)
        .ok()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return;
    };
    println!("{}", Access::of(pid).line());
    println!("HARDEN-TOKEN {}", probe::token_state());
    println!("HARDEN-TOKEN {}", probe::token_report());
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
/// created with the descriptors passes it, an ordinary one does not, so a copy started without
/// them refuses to start.
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

/// A restricted-token probe (like Codex's sandbox) is denied `PROCESS_VM_READ`,
/// `PROCESS_DUP_HANDLE` and `WRITE_DAC` on a child of a hardened job, from the child's start.
#[test]
fn a_restricted_token_probe_is_denied_a_child_of_a_hardened_job() {
    let job = Job::new(Hardened::apply().unwrap()).unwrap();
    let mut child = hanging_child(&job);
    restricted_probe(child.id(), HANDLE(child.as_raw_handle()));
    assert!(job.kill());
    drop(child.wait());
}

/// A same-user probe without `SeDebugPrivilege` is denied `PROCESS_VM_READ`, `PROCESS_DUP_HANDLE`
/// and `WRITE_DAC` on a child of a hardened job, from the child's start.
#[test]
fn a_same_user_probe_without_sedebug_is_denied_a_child_of_a_hardened_job() {
    let job = Job::new(Hardened::apply().unwrap()).unwrap();
    let mut child = hanging_child(&job);
    same_user_probe(child.id(), HANDLE(child.as_raw_handle()));
    assert!(job.kill());
    drop(child.wait());
}

/// A restricted-token probe (like Codex's sandbox) is denied `PROCESS_VM_READ`,
/// `PROCESS_DUP_HANDLE` and `WRITE_DAC` on the guard's own process once it hardened itself.
#[test]
fn a_restricted_token_probe_is_denied_the_guards_own_hardened_process() {
    let hardened = Hardened::apply().unwrap();
    // SAFETY: the pseudo handle of the current process needs no closing.
    restricted_probe(std::process::id(), unsafe { GetCurrentProcess() });
    drop(hardened);
}

/// A same-user probe without `SeDebugPrivilege` is denied `PROCESS_VM_READ`, `PROCESS_DUP_HANDLE`
/// and `WRITE_DAC` on the guard's own process once it hardened itself.
#[test]
fn a_same_user_probe_without_sedebug_is_denied_the_guards_own_hardened_process() {
    let hardened = Hardened::apply().unwrap();
    // SAFETY: the pseudo handle of the current process needs no closing.
    same_user_probe(std::process::id(), unsafe { GetCurrentProcess() });
    drop(hardened);
}
