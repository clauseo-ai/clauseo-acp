//! Windows: a child started through [`Job::spawn`] is in the job and running from its start,
//! with its arguments, environment and folder as given; its pipes carry lines both ways; and the
//! job's end ends it. An inheritable handle outside the handle list stays out of the child, and
//! a relative program or a batch file is refused. A child created in the guard's job stays in
//! the job of the process that creates it too (the agent's), and that job's end ends it. The
//! children are this crate's examples (`examples/env_stub.rs`, `examples/echo_stub.rs`), which
//! cargo builds for the tests, and this test binary itself as the agent's stand-in.
#![expect(
    unsafe_code,
    reason = "IsProcessInJob, an inheritable event looked up in a child's handle table, and a \
              child of a child opened by pid"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::Foundation::{
    CloseHandle, CompareObjectHandles, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::JobObjects::{
    IsProcessInJob, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    QueryInformationJobObject,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::core::{BOOL, PCWSTR};

use super::{Child, Io, Job, Launch};
use crate::write;

/// A built example, found from the test binary (`target/<profile>/deps/`) as
/// `tests/guard/support.rs` finds them.
fn example(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().and_then(Path::parent).unwrap();
    let path = profile.join("examples").join(format!("{name}.exe"));
    assert!(
        path.is_file(),
        "{} is missing; cargo test builds the examples",
        path.display()
    );
    path
}

/// A new empty folder for `test`, whose name has a space in it.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-job {test}-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    write::create_dir_all(&dir).unwrap();
    dir
}

fn in_job(process: RawHandle, job: &Job) -> bool {
    let mut inside = BOOL::default();
    // SAFETY: `process` is open while its owner lives, as is `job`'s handle, and `inside` is a
    // live BOOL.
    unsafe { IsProcessInJob(HANDLE(process), Some(job.handle()), &raw mut inside) }.unwrap();
    inside.as_bool()
}

/// `echo_stub <dir>/log hang`, piped both ways: it answers each request line and, at its
/// stdin's end, waits to be killed.
fn echo(job: &Job, dir: &Path) -> Child {
    let args = [dir.join("log").into_os_string(), OsString::from("hang")];
    job.spawn(&Launch {
        program: &example("echo_stub"),
        args: &args,
        cwd: dir,
        env: &BTreeMap::new(),
        stdin: Io::Piped,
        stdout: Io::Piped,
        stderr: Io::Inherit,
    })
    .unwrap()
}

#[test]
fn a_child_runs_in_the_job_with_its_arguments_environment_and_folder() {
    let dir = scratch("env");
    let job = Job::new().unwrap();
    let args: Vec<OsString> = [
        "",
        "two words",
        "\"quoted\"",
        r"trailing\",
        r#"a\\"b"#,
        r"--user-data-dir=C:\Users\a b\chrome-profile-1",
        "\u{e9}\u{3c0}",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let env: BTreeMap<String, OsString> = [
        ("CLAUSEO_JOB_TEST", "a b=\"c\" \u{e9}"),
        ("SystemRoot", r"C:\Windows"),
    ]
    .iter()
    .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
    .collect();
    let mut child = job
        .spawn(&Launch {
            program: &example("env_stub"),
            args: &args,
            cwd: &dir,
            env: &env,
            stdin: Io::Null,
            stdout: Io::Piped,
            stderr: Io::Null,
        })
        .unwrap();
    assert!(in_job(child.as_raw_handle(), &job));
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert!(child.wait().unwrap().success());
    let seen: Value = serde_json::from_str(out.trim()).unwrap();
    let expected: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(seen["args"], json!(expected));
    assert_eq!(seen["env"]["CLAUSEO_JOB_TEST"], "a b=\"c\" \u{e9}");
    assert_eq!(seen["env"]["SystemRoot"], r"C:\Windows");
    // Exactly the given environment: none of the test's own, such as its PATH.
    assert!(std::env::var_os("PATH").is_some());
    let names = seen["env"].as_object().unwrap();
    assert!(
        !names.keys().any(|name| name.eq_ignore_ascii_case("PATH")),
        "{seen}"
    );
    // The folder as given; Windows keeps the current folder's spelling.
    assert_eq!(Path::new(seen["cwd"].as_str().unwrap()), dir);
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn pipes_carry_lines_both_ways_and_closing_the_job_ends_the_child() {
    let dir = scratch("pipes");
    let job = Job::new().unwrap();
    let mut child = echo(&job, &dir);
    assert!(in_job(child.as_raw_handle(), &job));
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"ping"}}"#).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["received"]["method"], "ping");
    assert_eq!(child.try_wait().unwrap(), None);
    // The job's last handle closes (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE): the child ends, though
    // its stdin is still open. A child outside the job would hang here until nextest's timeout.
    drop(job);
    child.wait().unwrap();
    line.clear();
    assert_eq!(stdout.read_line(&mut line).unwrap(), 0, "{line}");
    drop(stdin);
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_jobs_end_and_kill_end_the_child_with_code_1() {
    let dir = scratch("kill");
    let job = Job::new().unwrap();
    let mut first = echo(&job, &dir);
    let mut second = echo(&job, &dir);
    let held = (first.stdin.take(), second.stdin.take());
    assert!(job.kill());
    assert_eq!(first.wait().unwrap().code(), Some(1));
    assert_eq!(second.wait().unwrap().code(), Some(1));
    // An ended child's kill is no error, as with std's.
    second.kill().unwrap();
    let job = Job::new().unwrap();
    let mut third = echo(&job, &dir);
    let _held_too = third.stdin.take();
    third.kill().unwrap();
    assert_eq!(third.wait().unwrap().code(), Some(1));
    drop(held);
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_program_that_cannot_start_is_an_error() {
    let dir = scratch("missing");
    let job = Job::new().unwrap();
    let missing = job.spawn(&Launch {
        program: &dir.join("missing.exe"),
        args: &[],
        cwd: &dir,
        env: &BTreeMap::new(),
        stdin: Io::Piped,
        stdout: Io::Piped,
        stderr: Io::Inherit,
    });
    assert!(missing.is_err());
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_relative_program_or_a_batch_file_is_refused() {
    let dir = scratch("refused");
    let job = Job::new().unwrap();
    let batch = dir.join("run.cmd");
    write::write(&batch, "@echo off\r\n").unwrap();
    let relative = PathBuf::from(r"examples\echo_stub.exe");
    for program in [relative, batch, dir.join("RUN.BAT. ")] {
        let refused = job.spawn(&Launch {
            program: &program,
            args: &[OsString::from("&calc")],
            cwd: &dir,
            env: &BTreeMap::new(),
            stdin: Io::Null,
            stdout: Io::Null,
            stderr: Io::Null,
        });
        let error = refused.unwrap_err();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::InvalidInput,
            "{program:?}: {error}"
        );
    }
    write::remove_dir_all(&dir).unwrap();
}

/// An event this process can hand to children: inheritable.
fn inheritable_event() -> OwnedHandle {
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap(),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    // SAFETY: `security` is a live SECURITY_ATTRIBUTES whose `nLength` is its size; no name.
    let raw = unsafe { CreateEventW(Some(&raw const security), true, false, PCWSTR::null()) };
    // SAFETY: the open handle CreateEventW just returned, which nothing else owns.
    unsafe { OwnedHandle::from_raw_handle(raw.unwrap().0) }
}

/// Whether `process` holds `ours`' object under `ours`' value, as an inherited handle does.
/// Another object under that value in the child is not it.
fn holds(process: RawHandle, ours: &OwnedHandle) -> bool {
    let ours = HANDLE(ours.as_raw_handle());
    let mut copy = HANDLE::default();
    // SAFETY: `process` is a child's process handle, open with every right (so with
    // PROCESS_DUP_HANDLE) while its owner lives; `ours` is open; `copy` is a live HANDLE.
    let copied = unsafe {
        let me = GetCurrentProcess();
        DuplicateHandle(
            HANDLE(process),
            ours,
            me,
            &raw mut copy,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if copied.is_err() {
        return false;
    }
    // SAFETY: both handles are open; `copy` is this function's own, closed once right after.
    let same = unsafe { CompareObjectHandles(copy, ours) }.as_bool();
    // SAFETY: as above.
    unsafe { CloseHandle(copy) }.unwrap();
    same
}

/// The handle list keeps every other inheritable handle out of a child. Std's `Command`, which
/// passes every inheritable handle on, shows the event is one and that `holds` sees it.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the control child, started with std's Command, which inherits every handle"
)]
fn an_inheritable_handle_outside_the_list_stays_out_of_the_child() {
    let dir = scratch("handles");
    let event = inheritable_event();
    let job = Job::new().unwrap();
    let mut listed = echo(&job, &dir);
    let held = listed.stdin.take();
    assert!(!holds(listed.as_raw_handle(), &event));
    let mut control = std::process::Command::new(example("echo_stub"))
        .args([dir.join("log").into_os_string(), OsString::from("hang")])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    assert!(holds(control.as_raw_handle(), &event));
    control.kill().unwrap();
    control.wait().unwrap();
    assert!(job.kill());
    listed.wait().unwrap();
    drop(held);
    write::remove_dir_all(&dir).unwrap();
}

/// What the agent's stand-in reads to know it is one: the scratch folder for its guard's child.
const AGENT: &str = "CLAUSEO_JOB_TEST_AGENT";
/// The line the stand-in prints, then the guard's child's pid.
const GUARD_CHILD: &str = "guard child ";

/// No test by itself (it returns at once): the agent's stand-in for
/// [`closing_the_agents_job_ends_the_child_made_in_the_guards_job`], which runs this binary
/// with [`AGENT`] set and only this test. It makes the guard's job, spawns a child in it through
/// the new path, checks it is there, prints its pid and waits for its stdin to end.
#[test]
fn agent_stand_in() {
    let Some(dir) = std::env::var_os(AGENT) else {
        return;
    };
    let guard = Job::new().unwrap();
    let mut child = echo(&guard, Path::new(&dir));
    let _held = child.stdin.take();
    assert!(in_job(child.as_raw_handle(), &guard));
    println!("{GUARD_CHILD}{}", child.id());
    std::io::stdout().flush().unwrap();
    // Until the test ends this process's job, or itself ends and closes the pipe.
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

/// Host code reads "the agent's tree is gone" as "the Chrome started in it is gone" (#532).
/// That holds only if a child created with the guard's job in its `JOB_LIST` also stays in the
/// job its creator is in (the agent's, no breakaway), as the old assign-after-start nesting did.
/// An outer job stands for the agent's (kill on close, no breakaway, as `clauseo-process`
/// makes it); a run of this binary in it stands for the guard. The test process stays outside
/// it, so the job's end cannot end the test.
#[test]
fn closing_the_agents_job_ends_the_child_made_in_the_guards_job() {
    let dir = scratch("nested");
    let outer = Job::new().unwrap();
    let env: BTreeMap<String, OsString> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value)))
        .filter(|(name, _)| name != AGENT)
        .chain([(AGENT.to_owned(), dir.clone().into_os_string())])
        .collect();
    let args = [
        "--exact",
        "browser_guard::job::tests::agent_stand_in",
        "--nocapture",
    ]
    .map(OsString::from);
    let mut agent = outer
        .spawn(&Launch {
            program: &std::env::current_exe().unwrap(),
            args: &args,
            cwd: &dir,
            env: &env,
            stdin: Io::Piped,
            stdout: Io::Piped,
            stderr: Io::Inherit,
        })
        .unwrap();
    let held = agent.stdin.take();
    let pid: u32 = BufReader::new(agent.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            line.strip_prefix(GUARD_CHILD)
                .map(|pid| pid.trim().parse().unwrap())
        })
        .expect("the stand-in names its child");
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    // SAFETY: opening a process by id takes no pointer; the handle is owned below. The child
    // cannot have exited yet: the stand-in holds its stdin open and waits on its own.
    let raw = unsafe { OpenProcess(access, false, pid) }.unwrap();
    // SAFETY: the open handle OpenProcess just returned, which nothing else owns.
    let child = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    assert!(
        in_job(child.as_raw_handle(), &outer),
        "the child left the agent's job"
    );
    // The outer job's last handle closes: the stand-in and the child it made in the guard's job
    // end. A child outside the outer job would hang here until nextest's timeout.
    drop(outer);
    // SAFETY: `child` is open with SYNCHRONIZE; the wait takes no pointer.
    let waited = unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), INFINITE) };
    assert_eq!(waited, WAIT_OBJECT_0);
    agent.wait().unwrap();
    drop(held);
    write::remove_dir_all(&dir).unwrap();
}

/// The job's limits, read back: it ends its processes when its last handle closes (0x2000), ends
/// a process at once when it crashes (0x400; its exit code the exception's, which the guard
/// reads, `chrome/postcheck/crashed.rs`), and lets none break away. The crash limit must be on
/// this job itself: the agent's job's does not reach a process in this nested one.
#[test]
fn the_job_kills_on_close_ends_a_crashing_process_and_allows_no_breakaway() {
    let job = Job::new().unwrap();
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap();
    // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes, the struct
    // this information class writes, and `job.handle()` the handle `job` owns.
    unsafe {
        QueryInformationJobObject(
            Some(job.handle()),
            JobObjectExtendedLimitInformation,
            (&raw mut limits).cast(),
            size,
            None,
        )
    }
    .unwrap();
    let flags = limits.BasicLimitInformation.LimitFlags;
    assert!(
        flags.contains(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE),
        "{flags:?}"
    );
    assert!(
        flags.contains(JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION),
        "{flags:?}"
    );
    assert!(!flags.contains(JOB_OBJECT_LIMIT_BREAKAWAY_OK), "{flags:?}");
    assert!(
        !flags.contains(JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK),
        "{flags:?}"
    );
}
