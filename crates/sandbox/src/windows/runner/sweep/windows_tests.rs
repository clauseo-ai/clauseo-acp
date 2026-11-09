//! The sweep's listings on a real machine, made as the pool user (M1): a process of that user's
//! in session 0 whose DACL shuts the user out is named by `WTSEnumerateProcessesW` with the
//! user's SID, left unnamed by the Toolhelp list, and counted an escapee by the sweep at
//! [`Point::Tick`], never `Ok`.
//!
//! Why it needs a run: Microsoft writes that "the caller must be a member of the Administrators
//! group to enumerate processes that are running under a different user's context"
//! (`WTSEnumerateProcessesW`, Remarks), but `WTSEnumerateProcessesExW`'s Remarks say "under
//! another user session". Neither says whether a caller that isn't an administrator is given its
//! own processes in session 0, where a task or a service starts them. Were it not, such a process
//! would be [`Owner::None`] in both lists, never opened, and the sweep would end `Ok`.
//!
//! A test user stands in for the pool user (`ClauseoSbTT` and six hex digits). Its own
//! scheduled task, registered with its password (`TASK_LOGON_PASSWORD`, "run whether the user is
//! logged on or not"), starts `ping.exe`: a task's process runs in session 0, whatever session
//! this test runs in. The Task Scheduler grants the user "Log on as a batch job" as it registers
//! the task, as Microsoft documents; that grant stays on the machine with the account's SID.
//!
//! The lists are read by this test binary again, copied into a folder the user may read and run
//! as the user the way the outer starts the inner (`spawn::log_on`: `CreateProcessWithLogonW`,
//! unelevated, in this test's session), suspended and put in a kill-on-close job as the inner
//! is. The user's own environment gives the child no variable of this test's, so its part is
//! [`sweep_child`], an ignored test run alone by its exact name, which finds its orders beside
//! its exe ([`ORDER`]: the ping's pid) and writes what it found there ([`REPORT`]). As the user,
//! the child shuts the ping's DACL itself, as an escapee would ([`SHUT`]), then reads both lists
//! with the sweep's own calls (`OsProcesses`) and sweeps (`sweep_own`).
//!
//! The test prints both sessions. When its own is 0 too, the run covers the user's own session-0
//! process, not an inner in the lawyer's session with its escapee in session 0.
//!
//! Elevated, from CI's elevated step only (`CLAUSEO_ELEVATED_TESTS=1`): making an account and a
//! task as it needs it. The task, the account with its profile, and the folder are removed on
//! drop, a failed assertion included.
#![expect(
    unsafe_code,
    reason = "FFI to the Task Scheduler, a pid's session and the ping's DACL"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::Storage::FileSystem::WRITE_DAC;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::TaskScheduler::{
    IExecAction, ITaskFolder, ITaskService, TASK_ACTION_EXEC, TASK_CREATE_OR_UPDATE,
    TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_PASSWORD, TaskScheduler,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

use super::os::{OsProcesses, sweep_own};
use super::{Owner, Point, Processes as _, Seen};
use crate::test_support::TempDir;
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::record::current_user_sid;
use crate::windows::refresh::windows_tests::system32;
use crate::windows::runner::spawn::{assign, kill_on_close_job, log_on, resume};
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::pool_steps::windows_bits::Com;
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::sids::Sid;
use crate::windows::test_dacl::{elevated, protect_with};
use crate::windows::token::os::shut_object;

/// The child's test, by the name libtest gives it.
const CHILD: &str = "windows::runner::sweep::windows_tests::sweep_child";
/// The test binary's copy the child runs, in the test's folder.
const CHILD_EXE: &str = "sweep-child.exe";
/// Beside the child's exe: the ping's pid, written by the test.
const ORDER: &str = "order.txt";
/// Beside the child's exe: what the child found, a `key value` line each.
const REPORT: &str = "report.txt";
/// The ping's DACL as the child leaves it: `SYSTEM`'s alone, and `OWNER RIGHTS` with nothing,
/// so its owner, the user, keeps no implied `READ_CONTROL` or `WRITE_DAC`.
const SHUT: &str = "D:P(A;;GA;;;SY)(A;;0x0;;;OW)";
/// How many [`pause`]s the wait for the task's ping takes at most: 60 s, as its first logon
/// makes the user's profile.
const WAIT_STEPS: u32 = 240;
/// How long the child may take: its logon, two lists and a sweep of at most a minute.
const CHILD_WAIT: Duration = Duration::from_secs(180);

/// Whether the test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// A quarter of a second, between two looks.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait for the Task Scheduler's process, with no clock to inject"
)]
fn pause() {
    std::thread::sleep(Duration::from_millis(250));
}

/// The test user's task in the root folder, ended and deleted on drop.
struct Task {
    root: ITaskFolder,
    name: String,
}

impl Task {
    /// Registers a task as `user`, with its password, that runs `ping.exe` for ten minutes, and
    /// starts it. The password goes to the Task Scheduler in a `BSTR` that isn't wiped: the
    /// account is the test's own, with a random password, and deleted with the test.
    fn start(user: &TestUser) -> Task {
        let name = format!("ClauseoSweepWts{}{}", std::process::id(), random_hex(2));
        let ping = format!(r"{}\PING.EXE", system32());
        let none = VARIANT::default();
        // SAFETY: a registered in-process class and its interface, no outer object; then each
        // call is on a live interface the one before returned, with BSTRs and VARIANT values
        // that outlive it: empty ones connect to this machine as this process's user.
        let (service, root) = unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).unwrap();
            service.Connect(&none, &none, &none, &none).unwrap();
            let root = service.GetFolder(&BSTR::from("\\")).unwrap();
            (service, root)
        };
        let task = Task { root, name };
        // SAFETY: as above; flags 0 for NewTask and for the run's VARIANT, no parameters.
        unsafe {
            let definition = service.NewTask(0).unwrap();
            let principal = definition.Principal().unwrap();
            principal
                .SetUserId(&BSTR::from(user.name.as_str()))
                .unwrap();
            principal.SetLogonType(TASK_LOGON_PASSWORD).unwrap();
            let settings = definition.Settings().unwrap();
            let (no, yes) = (VARIANT_BOOL::from(false), VARIANT_BOOL::from(true));
            settings.SetRunOnlyIfNetworkAvailable(no).unwrap();
            settings
                .SetExecutionTimeLimit(&BSTR::from("PT10M"))
                .unwrap();
            settings
                .SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)
                .unwrap();
            settings.SetAllowDemandStart(yes).unwrap();
            settings.SetDisallowStartIfOnBatteries(no).unwrap();
            settings.SetStopIfGoingOnBatteries(no).unwrap();
            settings.SetEnabled(yes).unwrap();
            let action = definition
                .Actions()
                .unwrap()
                .Create(TASK_ACTION_EXEC)
                .unwrap();
            let exec: IExecAction = action.cast().unwrap();
            exec.SetPath(&BSTR::from(ping.as_str())).unwrap();
            exec.SetArguments(&BSTR::from("-n 600 127.0.0.1")).unwrap();
            let registered = task
                .root
                .RegisterTaskDefinition(
                    &BSTR::from(task.name.as_str()),
                    &definition,
                    TASK_CREATE_OR_UPDATE.0,
                    &VARIANT::from(user.name.as_str()),
                    &VARIANT::from(user.password.as_str()),
                    TASK_LOGON_PASSWORD,
                    &none,
                )
                .unwrap();
            registered.Run(&none).unwrap();
        }
        task
    }

    /// Its last run's result, for a failure's message.
    fn last_result(&self) -> String {
        // SAFETY: a live folder and a BSTR that outlives the call.
        match unsafe { self.root.GetTask(&BSTR::from(self.name.as_str())) } {
            // SAFETY: a live task.
            Ok(task) => match unsafe { task.LastTaskResult() } {
                Ok(code) => format!("0x{code:08x}"),
                Err(error) => format!("unread: {error}"),
            },
            Err(error) => format!("no task: {error}"),
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        let name = BSTR::from(self.name.as_str());
        // SAFETY: a live folder, a BSTR that outlives each call; flags 0. The Task Scheduler
        // ends the ping as SYSTEM, whom its shut DACL still grants everything.
        unsafe {
            if let Ok(registered) = self.root.GetTask(&name)
                && let Err(error) = registered.Stop(0)
            {
                eprintln!("{} wasn't ended: {error}", self.name);
            }
            if let Err(error) = self.root.DeleteTask(&name, 0) {
                eprintln!("{} wasn't deleted: {error}", self.name);
            }
        }
    }
}

/// This test binary, copied into `dir` for the test user to run.
#[expect(
    clippy::disallowed_methods,
    reason = "copies this test binary into the test's own folder, for its test user to run"
)]
fn copy_self(dir: &Path) -> PathBuf {
    let exe = dir.join(CHILD_EXE);
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    exe
}

/// The pid of `user`'s `ping.exe`, as this administrator's WTS list names it, once the task
/// started it; panics with the task's last result if it doesn't within 60 s.
fn wait_for_ping(user: &Sid, task: &Task) -> u32 {
    let me = current_user_sid().unwrap();
    let mut calls = OsProcesses { me: &me };
    let theirs = Owner::User(user.clone());
    for _ in 0..WAIT_STEPS {
        let named = calls.named().unwrap();
        let ping = named
            .iter()
            .find(|seen| seen.image.eq_ignore_ascii_case("PING.EXE") && seen.owner == theirs);
        if let Some(seen) = ping {
            return seen.pid;
        }
        pause();
    }
    panic!(
        "the task's ping.exe didn't start as the test user within 60 s (last result {})",
        task.last_result()
    );
}

/// Runs [`sweep_child`] from `exe` as `user`, as the outer starts the inner, in a kill-on-close
/// job, and gives its report (or why there is none).
fn run_child(user: &TestUser, exe: &Path, dir: &Path) -> String {
    let job = kill_on_close_job().unwrap();
    let args = format!("--exact {CHILD} --ignored --nocapture --test-threads=1");
    let child = log_on(
        &user.name,
        user.password.clone(),
        exe.to_str().unwrap(),
        &args,
        dir.to_str().unwrap(),
    )
    .unwrap_or_else(|code| panic!("CreateProcessWithLogonW (needs Secondary Logon): {code}"));
    if let Err(error) = assign(&job, child.handle.0) {
        child.kill(1);
        panic!("AssignProcessToJobObject: {error}");
    }
    resume(&child).unwrap();
    let code = child.wait(Some(CHILD_WAIT)).unwrap();
    eprintln!("the child's exit code: {code:?}");
    // The job's one handle: closing it ends a child still running.
    drop(job);
    match std::fs::read_to_string(dir.join(REPORT)) {
        Ok(report) => report,
        Err(error) => format!("(no report: {error})"),
    }
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_session_0_process_that_shut_its_user_out_is_named_and_left_an_escapee() {
    if !gate("elevated_a_session_0_process_that_shut_its_user_out_is_named_and_left_an_escapee") {
        return;
    }
    // 1. The pool user's stand-in, and a folder it may read holding this binary.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    eprintln!("1. the test user: {} {}", user.name, user.sid.as_str());
    let dir = TempDir::new("sweep-wts");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let exe = copy_self(dir.path());

    // 2. The user's task starts ping in session 0. Dropped before the user, so the account
    // outlives its task's clean-up.
    let _com = Com::new();
    let task = Task::start(&user);
    let pid = wait_for_ping(&user.sid, &task);
    eprintln!("2. the task's ping.exe: pid {pid}");

    // 3. As the user, unelevated: the child shuts the ping, reads both lists and sweeps.
    dir.file(ORDER, &pid.to_string());
    let report = run_child(&user, &exe, dir.path());
    eprintln!("3. the child's report:\n{report}");
    let line = |key: &str| {
        report
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
            .unwrap_or("(missing)")
            .to_owned()
    };

    // 4. Session 0, shut to the user, named by WTS with the user's SID, an escapee.
    assert_eq!(
        line("user"),
        user.sid.as_str(),
        "the child ran as someone else"
    );
    assert_eq!(
        line("target-session"),
        "0",
        "the task's ping isn't in session 0"
    );
    if line("own-session") == "0" {
        eprintln!("4. this test's session is 0 too: an inner in another session isn't covered");
    }
    assert_eq!(line("shut"), "ok", "the user couldn't shut its own process");
    assert_eq!(
        line("listed"),
        "none",
        "the Toolhelp list read the ping's user: its DACL didn't shut the user out"
    );
    assert_eq!(
        line("named"),
        user.sid.as_str(),
        "M1: WTSEnumerateProcessesW didn't give the user its own session-0 process"
    );
    let sweep = line("sweep");
    let escapees = sweep
        .strip_prefix("escapees ")
        .and_then(|rest| rest.split(' ').next()?.parse::<usize>().ok());
    assert!(
        escapees >= Some(1),
        "the sweep didn't count an escapee: {sweep}"
    );
    let named = format!("process {pid} (");
    assert!(
        report
            .lines()
            .any(|line| line.starts_with("log ") && line.contains(&named)),
        "no line of the sweep's names pid {pid}"
    );
    drop(task);
    drop(user);
}

/// The child's part of the test above, a pass in any other run: with [`ORDER`] beside its exe,
/// it does [`child_report`]'s steps as the test user and writes the lines to [`REPORT`].
#[test]
#[ignore = "the elevated test's child: run by that test alone, as its test user"]
fn sweep_child() {
    let exe = std::env::current_exe().unwrap();
    let Some(dir) = exe.parent() else {
        return;
    };
    let Ok(order) = std::fs::read_to_string(dir.join(ORDER)) else {
        return;
    };
    let pid: u32 = order.trim().parse().unwrap();
    write_report(&dir.join(REPORT), &child_report(pid));
}

/// Writes the child's report.
#[expect(
    clippy::disallowed_methods,
    reason = "the child's report, in the test's own folder"
)]
fn write_report(path: &Path, report: &str) {
    std::fs::write(path, report).unwrap();
}

/// As the test user: its SID, this process's session and `pid`'s, then `pid`'s DACL shut
/// ([`SHUT`]), its user as each list gives it, and the sweep at [`Point::Tick`] with its lines.
fn child_report(pid: u32) -> String {
    let me = match current_user_sid() {
        Ok(me) => me,
        Err(error) => return format!("user unread: {error}"),
    };
    let mut lines = vec![
        format!("user {}", me.as_str()),
        format!("own-session {}", session(std::process::id())),
        format!("target-session {}", session(pid)),
    ];
    if let Err(error) = shut(pid) {
        lines.push(format!("shut {error}"));
        return lines.join("\n");
    }
    lines.push("shut ok".to_owned());
    let mut calls = OsProcesses { me: &me };
    lines.push(format!("listed {}", owner_of(calls.list(), pid)));
    lines.push(format!("named {}", owner_of(calls.named(), pid)));
    let mut log = Vec::new();
    let swept = sweep_own(&me, Point::Tick, &mut |line| log.push(line.to_owned()));
    let verdict = match &swept {
        Ok(()) => "ok".to_owned(),
        Err(error) => match error.escapees() {
            Some(escapees) => format!("escapees {} {error}", escapees.count),
            None => format!("failed {error}"),
        },
    };
    lines.push(format!("sweep {verdict}"));
    lines.extend(log.iter().map(|line| format!("log {line}")));
    lines.join("\n")
}

/// `pid`'s session (`ProcessIdToSessionId`), or why it couldn't be read.
fn session(pid: u32) -> String {
    let mut id = 0u32;
    // SAFETY: a plain pid and a live out-pointer.
    match unsafe { ProcessIdToSessionId(pid, &raw mut id) } {
        Ok(()) => id.to_string(),
        Err(error) => format!("unread: {error}"),
    }
}

/// Gives `pid`, a process of this user's, the DACL [`SHUT`].
fn shut(pid: u32) -> io::Result<()> {
    // SAFETY: a plain open by pid; the handle is owned by `Handle`.
    let process = unsafe { OpenProcess(PROCESS_ACCESS_RIGHTS(WRITE_DAC.0), false, pid) }
        .map_err(|e| os_error(&e))?;
    let process = Handle(process);
    shut_object(process.0, SHUT)
}

/// `pid`'s user in `listed`: its SID, `none`, `absent`, or the list's error.
fn owner_of(listed: io::Result<Vec<Seen>>, pid: u32) -> String {
    match listed {
        Err(error) => format!("unread: {error}"),
        Ok(seen) => match seen.iter().find(|seen| seen.pid == pid) {
            None => "absent".to_owned(),
            Some(Seen {
                owner: Owner::None, ..
            }) => "none".to_owned(),
            Some(Seen {
                owner: Owner::User(sid),
                ..
            }) => sid.as_str().to_owned(),
        },
    }
}
