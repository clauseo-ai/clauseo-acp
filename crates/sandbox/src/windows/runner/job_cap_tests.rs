//! The job's process cap on Windows, for real: a job capped at [`CAP`], with the outer's
//! completion port and UI limits, takes a nested job with no UI limits (as libuv's, Node's, is)
//! and counts its processes towards its cap, refuses a nested job with UI limits (error 50),
//! refuses a process assigned past its cap and the next one of its own starts, holds exactly
//! [`CAP`] after a refusal, and posts `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` to its port each time.
//!
//! The nesting rule is the VM's (probe 2, Windows Server 2025 build 26100, the values read back
//! from the jobs, the same as admin and as the lawyer): error 50 (`ERROR_NOT_SUPPORTED`) comes
//! only when a job with UI limits is nested in a job with UI limits. A capped job with UI `0xFF`
//! took libuv's `0x3C00` job with no UI limits; it refused a `0x2008` job with UI `0xFF`, and
//! so did a `0x2000` job with UI `0xFF` under another, so the cap plays no part. CI's run
//! 42575462912 gave 50 for this test's first version, which nested a UI-limited job.

use std::io::{Read as _, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, Stdio};

use windows::Win32::Foundation::HANDLE;

use super::job_port::ACTIVE_PROCESS_LIMIT;
use super::job_port::os::associate;
use super::spawn::{
    active_processes, assign, capped_job, kill_on_close_job, limits_of, own_job_shape, plain_job,
};

/// The test job's cap.
const CAP: u32 = 4;
/// `DETACHED_PROCESS`: no console, so no conhost joins the job beside each `cmd`.
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// `CREATE_SUSPENDED`: the refused process never runs a line.
const CREATE_SUSPENDED: u32 = 0x0000_0004;
/// `ERROR_NOT_SUPPORTED`: a job with UI limits can't nest in a job with UI limits.
const NOT_SUPPORTED: i32 = 50;
/// How long the port may stay silent before the test gives up on its message, in ms.
const PORT_WAIT: u32 = 10_000;

/// `cmd /d /c <script>`, detached, its stdin and stdout piped.
fn cmd(script: &str) -> Child {
    cmd_with(script, DETACHED_PROCESS)
}

/// `cmd /d /c <script>` with `flags`, its stdin and stdout piped.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts cmd processes to fill a capped job; each waits on its stdin"
)]
fn cmd_with(script: &str, flags: u32) -> Child {
    Command::new("cmd.exe")
        .raw_arg(format!("/d /c \"{script}\""))
        .creation_flags(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// A `cmd` that waits for a line on stdin, then exits.
fn waiter() -> Child {
    cmd("set /p x=")
}

/// A suspended `cmd`, never resumed: the job refuses and ends it.
fn suspended() -> Child {
    cmd_with("exit", DETACHED_PROCESS | CREATE_SUSPENDED)
}

fn handle(child: &Child) -> HANDLE {
    HANDLE(child.as_raw_handle())
}

/// The test process's own job, if any, for the log (CI shows it: `.config/nextest.toml`).
fn own_job() -> String {
    match own_job_shape() {
        Ok(None) => "the test process is in no job".to_owned(),
        Ok(Some((flags, ui))) => {
            format!("the test process's job: LimitFlags 0x{flags:x}, UI 0x{ui:x}")
        }
        Err(error) => format!("the test process's job can't be read: {error}"),
    }
}

/// Records the test process's own job: whether a test sits in an outer job bears on what the
/// runner meets when Clauseo starts inside another product's job.
#[test]
fn the_test_process_job_is_recorded() {
    let said = own_job();
    eprintln!("job_cap_tests: {said}");
    assert!(!said.contains("can't be read"), "{said}");
}

#[test]
fn a_capped_job_nests_a_plain_job_and_counts_it_refuses_the_next_and_tells_its_port() {
    let outer = own_job();
    let job = capped_job(CAP).unwrap();
    let limits = limits_of(&job).unwrap().BasicLimitInformation;
    assert_eq!(limits.ActiveProcessLimit, CAP);
    let mut port = associate(&job).unwrap().waiting(PORT_WAIT);
    // The first two go in the capped job, then in `nested`, a job with no UI limits, which so
    // nests in the capped job, as Node's libuv job does; they still count towards the cap.
    let nested = plain_job().unwrap();
    let first = waiter();
    assign(&job, handle(&first)).unwrap();
    assign(&nested, handle(&first)).unwrap_or_else(|e| panic!("{e}; {outer}"));
    let second = waiter();
    assign(&job, handle(&second)).unwrap();
    assign(&nested, handle(&second)).unwrap_or_else(|e| panic!("{e}; {outer}"));
    assert_eq!(active_processes(&nested).unwrap(), 2);
    assert_eq!(active_processes(&job).unwrap(), 2);
    // The third goes in the capped job; a job with UI limits refuses it (the agent's own job's
    // shape, nested: probe 2's P4), and it stays in the capped job's count.
    let ui_nested = kill_on_close_job().unwrap();
    let third = waiter();
    assign(&job, handle(&third)).unwrap();
    let moved = assign(&ui_nested, handle(&third));
    assert_eq!(
        moved.as_ref().map_err(std::io::Error::raw_os_error),
        Err(Some(NOT_SUPPORTED)),
        "{moved:?}; {outer}"
    );
    assert_eq!(active_processes(&ui_nested).unwrap(), 0);
    assert_eq!(active_processes(&job).unwrap(), 3);
    // The fourth fills the cap; once told, it tries to start one more.
    let mut fourth = cmd("set /p x= & cmd /d /c echo spawned");
    assign(&job, handle(&fourth)).unwrap();
    assert_eq!(active_processes(&job).unwrap(), CAP);
    // A fifth, assigned straight in, is refused and ended: Windows counts a newcomer and refuses
    // it when the count passes the limit, so the job admits at most CAP. These are detached, so
    // no console host joins beside them (the VM's agent job listed 257 against 256 with one;
    // 0049). Once the fifth has gone the job holds exactly CAP.
    let mut fifth = suspended();
    let refused = assign(&job, handle(&fifth));
    assert!(refused.is_err(), "the job took a fifth process");
    // Windows ends it with the refusal; the kill only makes sure the wait can't hang.
    drop(fifth.kill());
    drop(fifth.wait());
    assert_eq!(active_processes(&job).unwrap(), CAP);
    let mut seen = Vec::new();
    assert!(
        port.by_ref().any(|message| {
            seen.push(message);
            message == ACTIVE_PROCESS_LIMIT
        }),
        "no ACTIVE_PROCESS_LIMIT on the port for the fifth; it said {seen:?}"
    );
    let mut go = fourth.stdin.take().unwrap();
    go.write_all(b"go\r\n").unwrap();
    drop(go);
    let mut said = String::new();
    fourth
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut said)
        .unwrap();
    assert!(
        !said.contains("spawned"),
        "the job started a fifth process: {said}"
    );
    seen.clear();
    let limited = port.by_ref().any(|message| {
        seen.push(message);
        message == ACTIVE_PROCESS_LIMIT
    });
    assert!(
        limited,
        "no ACTIVE_PROCESS_LIMIT on the port; it said {seen:?}"
    );
    // Closing the jobs' only handles ends what is left in them.
    drop((job, nested, ui_nested));
    for mut child in [first, second, third, fourth] {
        drop(child.wait());
    }
}
