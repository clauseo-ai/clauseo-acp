//! The job's process cap on Windows, for real: a job capped at [`CAP`], with the outer's
//! completion port, counts a nested job's processes towards its cap, refuses the next process
//! one of its own starts, and posts `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` to its port.

use std::io::{Read as _, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Child, Command, Stdio};

use windows::Win32::Foundation::HANDLE;

use super::job_port::ACTIVE_PROCESS_LIMIT;
use super::job_port::os::associate;
use super::spawn::{active_processes, assign, capped_job, kill_on_close_job, limits_of};

/// The test job's cap.
const CAP: u32 = 4;
/// `DETACHED_PROCESS`: no console, so no conhost joins the job beside each `cmd`.
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// How long the port may stay silent before the test gives up on its message, in ms.
const PORT_WAIT: u32 = 10_000;

/// `cmd /d /c <script>`, detached, its stdin and stdout piped.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts cmd processes to fill a capped job; each waits on its stdin"
)]
fn cmd(script: &str) -> Child {
    Command::new("cmd.exe")
        .raw_arg(format!("/d /c \"{script}\""))
        .creation_flags(DETACHED_PROCESS)
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

fn handle(child: &Child) -> HANDLE {
    HANDLE(child.as_raw_handle())
}

#[test]
fn a_capped_job_counts_a_nested_jobs_processes_refuses_the_next_and_tells_its_port() {
    let job = capped_job(CAP).unwrap();
    let limits = limits_of(&job).unwrap().BasicLimitInformation;
    assert_eq!(limits.ActiveProcessLimit, CAP);
    let mut port = associate(&job).unwrap().waiting(PORT_WAIT);
    let nested = kill_on_close_job().unwrap();
    // The first goes in the capped job, then in `nested`, which so nests in the capped job; the
    // second goes straight into `nested`, and so into the capped job too.
    let first = waiter();
    assign(&job, handle(&first)).unwrap();
    assign(&nested, handle(&first)).unwrap();
    let second = waiter();
    assign(&nested, handle(&second)).unwrap();
    let third = waiter();
    assign(&job, handle(&third)).unwrap();
    // The fourth fills the cap; once told, it tries to start one more.
    let mut fourth = cmd("set /p x= & cmd /d /c echo spawned");
    assign(&job, handle(&fourth)).unwrap();
    assert_eq!(active_processes(&nested).unwrap(), 2);
    assert_eq!(active_processes(&job).unwrap(), CAP);
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
    let mut seen = Vec::new();
    let limited = port.by_ref().any(|message| {
        seen.push(message);
        message == ACTIVE_PROCESS_LIMIT
    });
    assert!(
        limited,
        "no ACTIVE_PROCESS_LIMIT on the port; it said {seen:?}"
    );
    // Closing both jobs' only handles ends what is left in them.
    drop((job, nested));
    for mut child in [first, second, third, fourth] {
        drop(child.wait());
    }
}
