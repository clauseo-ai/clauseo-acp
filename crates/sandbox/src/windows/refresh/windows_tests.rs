//! The tasks' descriptor on a real machine: the owner, holding `FRFX` on a task
//! ([`task_sddl`]), may start it with `schtasks /run` and end a running one with
//! `schtasks /end`. Run needs execute access: with `FR` alone, `/run` was refused ("Access is
//! denied", CI run 42416074600), so the owner holds execute, which also ends a task. That is
//! accepted: an ended stop never writes `StopCompleted` (`setup::stop`), and an ended refresh
//! keeps the old copy.
//!
//! Elevated, from CI's elevated step only (`CLAUSEO_ELEVATED_TESTS=1`): it makes a standard
//! account (`ClauseoSbTT` and six hex digits), a test folder `\ClauseoTest<pid>` with the
//! production folder's descriptor, and two LOCAL SYSTEM tasks in it with the production tasks'
//! descriptor, registered through `ITaskService` as `task.rs` does (`register_task` names its
//! own folder and action). The account runs `schtasks` through the Secondary Logon service,
//! unelevated. Everything is removed on drop, a failed assertion included.
#![expect(
    unsafe_code,
    reason = "FFI to the Task Scheduler and to start a process as the test user"
)]

use std::iter::once;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, VARIANT_BOOL, WAIT_OBJECT_0};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_SERVICE_ACCOUNT,
    TASK_RUNLEVEL_HIGHEST, TASK_STATE_RUNNING,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessWithLogonW, GetExitCodeProcess, LOGON_WITH_PROFILE,
    PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HSTRING, Interface, PCWSTR, PWSTR, w};
use zeroize::Zeroizing;

use super::task::{Apartment, service};
use super::task_sddl;
use crate::test_support::TempDir;
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::test_dacl::{elevated, protect_with};

/// LOCAL SYSTEM, the tasks' principal.
const SYSTEM: &str = "S-1-5-18";
/// How long a command run as the test user may take, in milliseconds: its first logon makes
/// its profile.
const COMMAND_MS: u32 = 120_000;
/// How many [`pause`]s a wait takes at most: 15 s.
const WAIT_STEPS: u32 = 60;

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
    reason = "a bounded wait for the Task Scheduler's run, with no clock to inject"
)]
fn pause() {
    std::thread::sleep(std::time::Duration::from_millis(250));
}

/// `%SystemRoot%\System32`.
fn system32() -> String {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    format!(r"{system}\System32")
}

/// Runs `script` in `cmd` as `user`, with its profile loaded, and waits for it: its exit code
/// (`None` if it didn't end in time) and what it printed, read from a file in `folder`, which
/// `user` must be able to write. `script` runs inside a `(...)` group: no bare parentheses.
fn try_in_cmd(user: &TestUser, folder: &Path, script: &str) -> (Option<u32>, String) {
    let output = folder.join("output.txt");
    let cmd = format!(r"{}\cmd.exe", system32());
    // `/s`: cmd takes what is between the outer quotes as it is.
    let line = format!(
        r#""{cmd}" /d /s /c "({script}) > "{}" 2>&1""#,
        output.display()
    );
    let mut line: Vec<u16> = line.encode_utf16().chain(once(0)).collect();
    let password: Zeroizing<Vec<u16>> =
        Zeroizing::new(user.password.encode_utf16().chain(once(0)).collect());
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap(),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every string is NUL-terminated and lives for the call; the command line is a
    // mutable buffer, as the call needs; `startup` and `info` are live.
    let started = unsafe {
        CreateProcessWithLogonW(
            &HSTRING::from(user.name.as_str()),
            w!("."),
            PCWSTR(password.as_ptr()),
            LOGON_WITH_PROFILE,
            &HSTRING::from(cmd.as_str()),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_NO_WINDOW,
            None,
            &HSTRING::from(system32()),
            &raw const startup,
            &raw mut info,
        )
    };
    drop(password);
    if let Err(error) = started {
        panic!("CreateProcessWithLogonW (needs the Secondary Logon service): {error}");
    }
    let mut code = None;
    // SAFETY: both handles came from CreateProcessWithLogonW and are closed once, here; only
    // this process is ended; `exit` is a live out-pointer.
    unsafe {
        if WaitForSingleObject(info.hProcess, COMMAND_MS) == WAIT_OBJECT_0 {
            let mut exit = 0u32;
            GetExitCodeProcess(info.hProcess, &raw mut exit).unwrap();
            code = Some(exit);
        } else if let Err(error) = TerminateProcess(info.hProcess, 1) {
            eprintln!("the test user's cmd wasn't ended: {error}");
        }
        drop(CloseHandle(info.hThread));
        drop(CloseHandle(info.hProcess));
    }
    let printed = match std::fs::read(&output) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => format!("(its output can't be read: {error})"),
    };
    (code, printed)
}

/// The test folder and its tasks: each task ended and deleted, then the folder, on drop.
struct Scheduled {
    root: ITaskFolder,
    folder: ITaskFolder,
    name: String,
    tasks: Vec<String>,
}

impl Scheduled {
    /// Makes `\<name>` with `sddl`, as `task_folder::prepare` makes `\Clauseo`.
    fn new(service: &ITaskService, name: &str, sddl: &str) -> Scheduled {
        // SAFETY: a live service; the BSTRs and the VARIANT outlive each call.
        let (root, folder) = unsafe {
            let root = service.GetFolder(&BSTR::from("\\")).unwrap();
            let folder = root
                .CreateFolder(&BSTR::from(name), &VARIANT::from(sddl))
                .unwrap();
            (root, folder)
        };
        Scheduled {
            root,
            folder,
            name: name.to_owned(),
            tasks: Vec::new(),
        }
    }

    /// Registers `task` in the folder: LOCAL SYSTEM runs `cmd.exe` with `arguments`, on demand
    /// only, as `register_task` sets its tasks up, with `sddl` as its descriptor.
    fn register(&mut self, service: &ITaskService, task: &str, arguments: &str, sddl: &str) {
        self.tasks.push(task.to_owned());
        let cmd = format!(r"{}\cmd.exe", system32());
        // SAFETY: each call is on a live interface the one before returned, with BSTRs and
        // VARIANT values that outlive it; flags 0 for NewTask.
        unsafe {
            let definition = service.NewTask(0).unwrap();
            let principal = definition.Principal().unwrap();
            principal.SetUserId(&BSTR::from(SYSTEM)).unwrap();
            principal.SetLogonType(TASK_LOGON_SERVICE_ACCOUNT).unwrap();
            principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST).unwrap();
            let settings = definition.Settings().unwrap();
            let (no, yes) = (VARIANT_BOOL::from(false), VARIANT_BOOL::from(true));
            settings.SetRunOnlyIfNetworkAvailable(no).unwrap();
            settings.SetExecutionTimeLimit(&BSTR::from("PT2M")).unwrap();
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
            exec.SetPath(&BSTR::from(cmd.as_str())).unwrap();
            exec.SetArguments(&BSTR::from(arguments)).unwrap();
            self.folder
                .RegisterTaskDefinition(
                    &BSTR::from(task),
                    &definition,
                    TASK_CREATE_OR_UPDATE.0,
                    &VARIANT::from(SYSTEM),
                    &VARIANT::default(),
                    TASK_LOGON_SERVICE_ACCOUNT,
                    &VARIANT::from(sddl),
                )
                .unwrap();
        }
    }

    /// The task `task` in the folder, read afresh.
    fn task(&self, task: &str) -> IRegisteredTask {
        // SAFETY: a live folder and a BSTR that outlives the call.
        unsafe { self.folder.GetTask(&BSTR::from(task)) }.unwrap()
    }

    /// Whether `task` is running: its state, and how many instances it has.
    fn running(&self, task: &str) -> (bool, i32) {
        let task = self.task(task);
        // SAFETY: a live task; flags 0.
        unsafe {
            let state = task.State().unwrap();
            let count = task.GetInstances(0).unwrap().Count().unwrap();
            (state == TASK_STATE_RUNNING, count)
        }
    }
}

impl Drop for Scheduled {
    fn drop(&mut self) {
        for task in &self.tasks {
            let name = BSTR::from(task.as_str());
            // SAFETY: a live folder, a BSTR that outlives each call; flags 0.
            unsafe {
                if let Ok(registered) = self.folder.GetTask(&name)
                    && let Err(error) = registered.Stop(0)
                {
                    eprintln!("{task} wasn't ended: {error}");
                }
                if let Err(error) = self.folder.DeleteTask(&name, 0) {
                    eprintln!("{task} wasn't deleted: {error}");
                }
            }
        }
        // SAFETY: a live root folder and a BSTR naming its child; flags 0.
        if let Err(error) = unsafe { self.root.DeleteFolder(&BSTR::from(self.name.as_str()), 0) } {
            eprintln!("\\{} wasn't deleted: {error}", self.name);
        }
    }
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_owner_can_start_and_end_a_task_it_can_read_and_execute() {
    if !gate("elevated_the_owner_can_start_and_end_a_task_it_can_read_and_execute") {
        return;
    }
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let dir = TempDir::new("owner-task-run");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        user.sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let marker = dir.path().join("ran");
    let sddl = task_sddl(&user.sid);
    eprintln!("the tasks' and the folder's descriptor: {sddl}");

    // Dropped last: COM stays up until every interface below is released.
    let _apartment = Apartment::enter().unwrap();
    let service = service().unwrap();
    let name = format!("ClauseoTest{}", std::process::id());
    let mut scheduled = Scheduled::new(&service, &name, &sddl);
    let short = format!(r#"/d /c echo x> "{}""#, marker.display());
    scheduled.register(&service, "A", &short, &sddl);
    scheduled.register(&service, "B", "/d /c ping -n 60 127.0.0.1 > nul", &sddl);
    let info = (OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0).cast_signed();
    // SAFETY: a live task; the owner and the DACL.
    let read_back = unsafe { scheduled.task("A").GetSecurityDescriptor(info) }.unwrap();
    eprintln!("task A's descriptor as registered: {read_back}");
    let schtasks = format!(r"{}\schtasks.exe", system32());

    // 3. The owner starts A through its read and execute entry.
    let run = format!(r#""{schtasks}" /run /tn \{name}\A"#);
    let (code, printed) = try_in_cmd(&user, dir.path(), &run);
    eprintln!("as the owner: {run}\nexit {code:?}\n{printed}");
    assert_eq!(code, Some(0), "schtasks /run as the owner:\n{printed}");
    let mut ran = marker.is_file();
    for _ in 0..WAIT_STEPS {
        if ran {
            break;
        }
        pause();
        ran = marker.is_file();
    }
    assert!(ran, "task A didn't write {} within 15 s", marker.display());

    // 4. This elevated process starts B; the owner ends it, as its execute access allows.
    // SAFETY: a live task and an empty VARIANT: no parameters.
    unsafe { scheduled.task("B").Run(&VARIANT::default()) }.unwrap();
    let mut running = scheduled.running("B");
    for _ in 0..WAIT_STEPS {
        if running.0 {
            break;
        }
        pause();
        running = scheduled.running("B");
    }
    eprintln!("task B started elevated: (running, instances) = {running:?}");
    assert!(running.0, "task B didn't start: {running:?}");
    let end = format!(r#""{schtasks}" /end /tn \{name}\B"#);
    let (code, printed) = try_in_cmd(&user, dir.path(), &end);
    eprintln!("as the owner: {end}\nexit {code:?}\n{printed}");
    assert_eq!(code, Some(0), "schtasks /end as the owner:\n{printed}");
    let mut after = scheduled.running("B");
    for _ in 0..WAIT_STEPS {
        if !after.0 && after.1 == 0 {
            break;
        }
        pause();
        after = scheduled.running("B");
    }
    eprintln!("task B after the owner's /end: (running, instances) = {after:?}");
    assert!(!after.0 && after.1 == 0, "task B still runs: {after:?}");
}
