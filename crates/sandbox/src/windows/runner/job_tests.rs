//! The launch's kill-on-close job on Windows: a process in it that reads its own job's pid list
//! with no job handle, as the inner runner's sweeps do, holds no handle to the job, so it dies
//! when the job's creator closes the job's one handle. The job's limits hold its process cap.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

use super::spawn::{assign, kill_on_close_job, limits_of, logon_startup};
use super::station::StationDesktop;
use super::sweep::os::own_job_pids;

/// Set in the child's environment: it makes [`job_child`] the child's part.
const JOB_CHILD: &str = "CLAUSEO_SANDBOX_TEST_JOB_CHILD";
/// How long the child has to die once the job's handle is closed.
const DIE_WAIT: Duration = Duration::from_secs(10);

/// The child's part of the next test, a pass in any other run. Once told `go` on stdin (it is
/// in the job by then), it reads its own job's pid list as the sweeps do, prints it, and waits
/// for the end of stdin, which the test keeps open until the job's end must have killed it.
#[test]
fn job_child() {
    if std::env::var_os(JOB_CHILD).is_none() {
        return;
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    let said = match own_job_pids() {
        Ok(pids) => {
            let pids: Vec<String> = pids.iter().map(ToString::to_string).collect();
            format!("ok {}", pids.join(","))
        }
        Err(error) => format!("error {error}"),
    };
    println!("job-child {} {said}", std::process::id());
    std::io::stdout().flush().unwrap();
    drop(std::io::stdin().read_line(&mut line));
}

/// This test binary again, running [`job_child`] alone with [`JOB_CHILD`] set.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts its own binary as the job's child, which waits on its stdin"
)]
fn start_child() -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "windows::runner::job_tests::job_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(JOB_CHILD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_kill_on_close_jobs_child_that_read_its_own_job_dies_with_the_creators_handle() {
    let job = kill_on_close_job().unwrap();
    let mut child = start_child();
    let pid = child.id();
    let assigned = assign(&job, HANDLE(child.as_raw_handle()));
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    if let Err(error) = assigned {
        drop(stdin);
        drop(child.wait());
        panic!("AssignProcessToJobObject: {error}");
    }
    stdin.write_all(b"go\n").unwrap();
    stdin.flush().unwrap();
    // Not at the line's start: with `--test-threads=1` libtest prints `test <name> ... ` before
    // the test runs, so the child's line follows it. A search for a line starting with it read
    // on to the end of the child's stdout, which never came, as the child waits for the end of
    // the stdin held here (run 42419661450 timed out so).
    let said = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            line.split_once("job-child ")
                .map(|(_, rest)| format!("job-child {rest}"))
        })
        .unwrap_or_default();
    // Its immediate job is the test's, holding the child alone, even when this test runs in a
    // job of its own (nested jobs).
    assert_eq!(said, format!("job-child {pid} ok {pid}"));
    let (tell, ended) = mpsc::channel();
    std::thread::spawn(move || drop(tell.send(child.wait().map(|status| status.code()))));
    // The job's one handle, the creator's: closing it ends the child, which still waits on the
    // stdin this test holds open.
    drop(job);
    let died = ended.recv_timeout(DIE_WAIT);
    drop(stdin);
    let code = died
        .expect("the child outlived the job's last handle")
        .unwrap();
    assert!(code.is_some(), "{code:?}");
}

/// `CreateProcessWithLogonW`'s startup info (the inner runner's) always names the agent's
/// station's desktop: never `NULL`, which would have the Secondary Logon service grant the pool
/// user's logon the lawyer's `WinSta0\Default`.
#[test]
fn every_logon_starts_on_the_agents_station_desktop() {
    let desktop = StationDesktop::of("Service-0x0-3e7$").unwrap();
    let mut wide = desktop.wide();
    let startup = logon_startup(&mut wide);
    assert!(!startup.lpDesktop.is_null());
    assert_eq!(startup.lpDesktop.0, wide.as_mut_ptr());
    let text = String::from_utf16(wide.split_last().unwrap().1).unwrap();
    assert_eq!(text, r"Service-0x0-3e7$\Default");
}

/// The launch's job holds at most 256 processes, and still ends them all when its handle
/// closes: its limits as Windows reads them back.
#[test]
fn the_job_caps_its_processes_at_256() {
    let limits = limits_of(&kill_on_close_job().unwrap())
        .unwrap()
        .BasicLimitInformation;
    assert!(limits.LimitFlags.contains(JOB_OBJECT_LIMIT_ACTIVE_PROCESS));
    assert!(
        limits
            .LimitFlags
            .contains(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)
    );
    assert_eq!(limits.ActiveProcessLimit, 256);
}
