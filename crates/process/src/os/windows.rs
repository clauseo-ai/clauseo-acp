//! The Windows half: the agent runs in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! which this process owns, so Windows ends the agent's tree when Clauseo dies, however it dies.
//! The agent starts suspended, joins the job, and only then runs, so nothing it starts can run
//! outside the job. It starts with `CREATE_NO_WINDOW`: Windows gives it a console with no
//! window, served by a `conhost.exe` that Windows starts as the agent's child, in the job. No
//! Ctrl-Break is sent to it (one reaches only processes that share the sender's console), so
//! Windows has no SIGTERM step. The job's process list is how the agent's descendants are
//! listed: every process it started is in it, an orphaned one too, and so is that conhost, which
//! every snapshot holds, so no sweep ends it. A console command the agent starts without a
//! console gets a conhost of its own, its child and new in the job, which a sweep ends with it.
//! Every Win32 call of the crate is in this file.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object, process and thread APIs"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use async_process::Child;
use windows::Win32::Foundation::{CloseHandle, ERROR_MORE_DATA, FILETIME, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, GetProcessTimes, OpenProcess, OpenThread,
    PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    ResumeThread, THREAD_SUSPEND_RESUME, TerminateProcess, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use crate::descendants::Proc;
use crate::{Error, SpawnOptions};

/// The exit code `TerminateJobObject` gives the processes it ends, and `terminate` the listed
/// processes it ends one by one: one nothing else uses (std's `Child::kill` gives 1), so a test
/// sees that the job ended the agent.
const TERMINATED: u32 = 0xC1A0;

/// Makes the agent start suspended and with no console window, plus any flags `opts` adds.
pub(crate) fn prepare(command: &mut Command, opts: &SpawnOptions) {
    command.creation_flags(CREATE_SUSPENDED.0 | CREATE_NO_WINDOW.0 | opts.creation_flags);
}

/// The agent's Job Object. Dropping it closes the handle, which ends what is left of the tree.
/// An `OwnedHandle`, so the tree is `Send + Sync` as on Unix, and closes once on drop. Only the
/// tree holds it strongly: a [`Root`] holds a `Weak`, so the job still closes with the tree.
#[derive(Debug)]
pub(crate) struct Tree {
    job: Arc<OwnedHandle>,
}

impl Tree {
    /// The tree of `child`, just started suspended from a `prepare`d command: a new job holds it,
    /// then its threads resume. When a step fails, the child is ended and the error returned; an
    /// agent never runs outside its job.
    pub(crate) fn adopt(child: &mut Child) -> Result<Tree, Error> {
        let tree = match Tree::new() {
            Ok(tree) => tree,
            Err(error) => {
                drop(child.kill());
                return Err(error);
            }
        };
        if let Err(error) = join(&tree, child.id()) {
            // Still suspended, so it started nothing: ending it ends the tree.
            drop(child.kill());
            return Err(error);
        }
        if let Err(error) = resume(child.id()) {
            tree.kill();
            return Err(error);
        }
        Ok(tree)
    }

    /// Ends every process in the job. Returns whether Windows took the call.
    pub(crate) fn kill(&self) -> bool {
        // SAFETY: `self.job()` is the job handle `self` owns, open until `self` drops.
        unsafe { TerminateJobObject(self.job(), TERMINATED) }.is_ok()
    }

    /// A tree whose kill Windows refuses: a new, empty job's handle with no access rights, so
    /// `TerminateJobObject` fails. For the test of the ladder's kill when the job will not end.
    #[cfg(test)]
    pub(crate) fn that_will_not_end() -> Tree {
        use windows::Win32::Foundation::{DUPLICATE_HANDLE_OPTIONS, DuplicateHandle};
        use windows::Win32::System::Threading::GetCurrentProcess;

        let full = Tree::new().expect("create a job");
        let mut bare = HANDLE::default();
        // SAFETY: `full.job()` is the job handle `full` owns, open for this call; `bare` is a
        // live HANDLE the call writes the duplicate to, owned below by the `OwnedHandle`.
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                full.job(),
                GetCurrentProcess(),
                &raw mut bare,
                0,
                false,
                DUPLICATE_HANDLE_OPTIONS(0),
            )
        }
        .expect("duplicate the job handle with no access");
        // SAFETY: `bare` is the open handle DuplicateHandle just returned, which nothing else
        // owns; the `OwnedHandle` closes it once, on drop.
        let job = Arc::new(unsafe { OwnedHandle::from_raw_handle(bare.0) });
        Tree { job }
    }

    /// The job handle, borrowed for a Win32 call.
    fn job(&self) -> HANDLE {
        raw(&self.job)
    }

    /// A new job that ends its processes when its last handle closes.
    fn new() -> Result<Tree, Error> {
        // SAFETY: no security attributes and no name are valid arguments; the handle returned is
        // owned by the `Tree` built from it, which closes it once.
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| os_error("could not create a job object", error))?;
        // SAFETY: `job` is the open handle CreateJobObjectW just returned, which nothing else owns
        // or closes; the `OwnedHandle` closes it once, on drop.
        let job = Arc::new(unsafe { OwnedHandle::from_raw_handle(job.0) });
        let tree = Tree { job };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size =
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|source| {
                Error::Os {
                    what: "the job limit struct is too large",
                    source: io::Error::other(source),
                }
            })?;
        // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes, the
        // struct this information class takes, and `tree.job()` is the job handle `tree` owns.
        unsafe {
            SetInformationJobObject(
                tree.job(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        }
        .map_err(|error| os_error("could not set the job to kill on close", error))?;
        Ok(tree)
    }
}

/// How many pids the first job query makes room for; a short answer is retried with more room,
/// up to [`MAX_JOB_PIDS`].
const FIRST_JOB_PIDS: usize = 64;
/// The most processes of one job the listing reads.
const MAX_JOB_PIDS: usize = 65_536;
/// The room, in UTF-16 units, for an executable path: a first try and the longest Windows path.
const IMAGE_ROOMS: [usize; 2] = [1_024, 32_768];

/// Where a listing of the agent's descendants starts: its job, held weakly so the job still
/// closes (ending its tree) when the [`Tree`] drops, and the agent's own pid, which is in the job
/// but is no descendant.
#[derive(Debug, Clone)]
pub(crate) struct Root {
    job: Weak<OwnedHandle>,
    pid: u32,
}

impl Root {
    /// The root of `tree`, whose agent is `pid`.
    pub(crate) fn of(tree: &Tree, pid: u32) -> Root {
        Root {
            job: Arc::downgrade(&tree.job),
            pid,
        }
    }

    /// Every running process in the agent's job but the agent itself, each with its parent pid
    /// from a Toolhelp snapshot (`None` when the snapshot fails or misses it); `None` when the
    /// job is gone or the query failed. A process that ends while it is read is left out.
    ///
    /// A pid from the job's list is opened, then kept only if the process it opened still runs
    /// and is in the job. The job lists a process that has exited while a handle to it is open
    /// (its parent's, until the parent closes it), so an ended process would otherwise look
    /// alive, and a sweep would count it again. In the job, because a process that ended after
    /// the list was read can have given its pid to a process outside the job. The handles stay
    /// open while the Toolhelp snapshot is taken, so no listed pid can be reused before its
    /// parent is read, and the parent read is that of the listed process.
    pub(crate) fn list(&self) -> Option<Vec<Proc>> {
        let job = self.job.upgrade()?;
        let pids = job_pids(&job)?;
        // Every handle is open before the Toolhelp snapshot below is taken.
        let mut opened: Vec<(u32, OwnedHandle, u64)> = Vec::with_capacity(pids.len());
        for pid in pids.into_iter().filter(|&pid| pid != self.pid) {
            let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)
            else {
                continue;
            };
            if !alive(&process) || !in_job(&process, &job) {
                continue;
            }
            if let Some(start) = created(&process) {
                opened.push((pid, process, start));
            }
        }
        let parents = parents();
        let listed = opened
            .into_iter()
            .map(|(pid, process, start)| Proc {
                pid,
                parent: parents
                    .iter()
                    .find(|&&(child, _)| child == pid)
                    .map(|&(_, parent)| parent),
                start,
                leads_group: false,
                exe: image(&process),
            })
            .collect();
        Some(listed)
    }
}

/// Now as the system time in 100 ns units since 1601: `GetSystemTimeAsFileTime`, which moves in
/// timer ticks and so is never ahead of a creation time read later. `None` for a zero time, which
/// no running system has.
pub(crate) fn now() -> Option<u64> {
    // SAFETY: the call takes no pointer from here and returns the FILETIME by value.
    let time = unsafe { GetSystemTimeAsFileTime() };
    Some((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)).filter(|&t| t > 0)
}

/// Whether process `pid` started before a cutoff taken now, for tests.
#[cfg(feature = "test-support")]
pub(crate) fn started_before_now(pid: u32) -> bool {
    let start = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).and_then(|p| created(&p));
    start.zip(now()).is_some_and(|(start, now)| start < now)
}

/// Process `pid`'s image path and parent pid, the handle open through the snapshot, for tests.
#[cfg(feature = "test-support")]
pub(crate) fn image_and_parent(pid: u32) -> Option<(PathBuf, u32)> {
    let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let parent = parents().into_iter().find(|&(child, _)| child == pid)?.1;
    Some((image(&process)?, parent))
}

/// Every process's (pid, parent pid) from a Toolhelp process snapshot; empty when the snapshot
/// fails. A parent pid can name a process that has exited, or a pid reused since.
fn parents() -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    if !each_process(|entry| found.push((entry.th32ProcessID, entry.th32ParentProcessID))) {
        found.clear();
    }
    found
}

/// `FILETIME`'s count (100 ns units since 1601) at the Unix epoch.
const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

/// The creation times of the running processes whose image name (`szExeFile`, `WINWORD.EXE`)
/// is `name`, compared ignoring ASCII case; an empty list when none runs. `None` when the
/// snapshot fails or a matching process cannot be opened for its creation time (it runs, but
/// when it started is unknown).
pub(crate) fn named(name: &str) -> Option<Vec<SystemTime>> {
    let mut pids = Vec::new();
    let listed = each_process(|entry| {
        let length = entry
            .szExeFile
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let image = String::from_utf16_lossy(entry.szExeFile.get(..length).unwrap_or_default());
        if image.eq_ignore_ascii_case(name) {
            pids.push(entry.th32ProcessID);
        }
    });
    if !listed {
        return None;
    }
    pids.into_iter()
        .map(|pid| {
            let created = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).and_then(|p| created(&p))?;
            let since = created.checked_sub(UNIX_EPOCH_FILETIME)?.checked_mul(100)?;
            SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(since))
        })
        .collect()
}

/// Calls `visit` with each entry of a Toolhelp process snapshot of the whole system; `false`
/// when the snapshot cannot be taken.
fn each_process(mut visit: impl FnMut(&PROCESSENTRY32W)) -> bool {
    let Ok(size) = u32::try_from(size_of::<PROCESSENTRY32W>()) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: a process snapshot of the whole system takes no pointer; the handle is owned
    // below and closed once.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    // SAFETY: `snapshot` is the open handle the call just returned, which nothing else owns.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
    // SAFETY: `snapshot` is a live process snapshot and `entry` a PROCESSENTRY32W whose
    // `dwSize` is set, as Process32FirstW and Process32NextW require.
    let mut next = unsafe { Process32FirstW(raw(&snapshot), &raw mut entry) };
    while next.is_ok() {
        visit(&entry);
        // SAFETY: as for Process32FirstW above.
        next = unsafe { Process32NextW(raw(&snapshot), &raw mut entry) };
    }
    true
}

/// Ends `process` with `TerminateProcess` if it still runs with the creation time it was listed
/// with. The handle opened for the check is the one the kill goes through, so a reused pid is
/// never ended. Returns that handle when it is the listed process's, ended or not: while it is
/// open, Windows gives the pid to no other process, so a process listed later with it as parent
/// was started by the listed one.
pub(crate) fn terminate(process: &Proc) -> Option<OwnedHandle> {
    let handle = open(
        process.pid,
        PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
    )?;
    if created(&handle) != Some(process.start) {
        return None;
    }
    // SAFETY: `handle` is a live process handle opened above with PROCESS_TERMINATE, owned by
    // this function until it returns it.
    unsafe { TerminateProcess(raw(&handle), TERMINATED) }.unwrap_or_default();
    Some(handle)
}

/// Whether `process` is in `job`; `false` when Windows does not say.
fn in_job(process: &OwnedHandle, job: &OwnedHandle) -> bool {
    let mut inside = windows::core::BOOL(0);
    // SAFETY: `process` is a live handle with PROCESS_QUERY_LIMITED_INFORMATION, `job` the live
    // job handle, and `inside` a live BOOL the call writes.
    let asked = unsafe { IsProcessInJob(raw(process), Some(raw(job)), &raw mut inside) };
    asked.is_ok() && inside.as_bool()
}

/// Whether process `pid` is still running, for tests: see [`alive`].
#[cfg(feature = "test-support")]
pub(crate) fn running(pid: u32) -> bool {
    open(pid, PROCESS_SYNCHRONIZE).is_some_and(|handle| alive(&handle))
}

/// Whether `process` still runs: a process handle is signalled once the process exits, so a
/// wait of 0 ms that times out means it has not. Unlike an exit code of `STILL_ACTIVE`, which a
/// process can also exit with, this cannot mistake an ended process for a running one.
fn alive(process: &OwnedHandle) -> bool {
    // SAFETY: `process` is a live process handle opened with PROCESS_SYNCHRONIZE, which a wait
    // needs; a 0 ms wait only reads its state.
    let waited = unsafe { WaitForSingleObject(raw(process), 0) };
    waited == WAIT_TIMEOUT
}

/// Ends `pid`, for a test that cleans up after a failed assert.
#[cfg(feature = "test-support")]
pub(crate) fn kill_pid(pid: u32) {
    if let Some(handle) = open(pid, PROCESS_TERMINATE) {
        // SAFETY: `handle` is a live process handle opened with PROCESS_TERMINATE.
        unsafe { TerminateProcess(raw(&handle), TERMINATED) }.unwrap_or_default();
    }
}

/// The ids of the processes in `job`, from `JobObjectBasicProcessIdList`; `None` when the
/// query fails, or the job holds more than [`MAX_JOB_PIDS`].
fn job_pids(job: &OwnedHandle) -> Option<Vec<u32>> {
    // The list's header (two u32 counts) in `usize` slots: the ids start at this slot.
    let header =
        std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    let mut room = FIRST_JOB_PIDS;
    loop {
        // `usize` slots, so the buffer has the struct's alignment; zeroed, so it is all
        // initialised whatever the call writes.
        let mut buffer: Vec<usize> = vec![0; header + room];
        let bytes = u32::try_from(buffer.len() * size_of::<usize>()).ok()?;
        // SAFETY: `buffer` is live, aligned for JOBOBJECT_BASIC_PROCESS_ID_LIST and `bytes`
        // bytes long, which is at least the struct's size (`room` >= 1); the call writes the
        // two counts and at most the ids that fit in `bytes`. `job` is the live job handle.
        let queried = unsafe {
            QueryInformationJobObject(
                Some(raw(job)),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                bytes,
                None,
            )
        };
        if let Err(error) = &queried
            && error.code() != ERROR_MORE_DATA.to_hresult()
        {
            return None;
        }
        // SAFETY: the buffer starts with the struct's two u32 counts (repr(C), at offset 0),
        // initialised either by the call or by the zeroing above.
        let (assigned, listed) = unsafe {
            let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            (
                (*list).NumberOfAssignedProcesses,
                (*list).NumberOfProcessIdsInList,
            )
        };
        let (assigned, listed) = (
            usize::try_from(assigned).ok()?,
            usize::try_from(listed).ok()?,
        );
        if queried.is_ok() && listed >= assigned {
            let ids = buffer.get(header..header + listed.min(room))?;
            return Some(
                ids.iter()
                    .filter_map(|&id| u32::try_from(id).ok())
                    .collect(),
            );
        }
        if room >= MAX_JOB_PIDS {
            return None;
        }
        room = (room * 2).max(assigned + 8).min(MAX_JOB_PIDS);
    }
}

/// Opens process `pid` with `access`; `None` when it is gone or refuses.
fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    // SAFETY: opening a process by id takes no pointer.
    let handle = unsafe { OpenProcess(access, false, pid) }.ok()?;
    // SAFETY: `handle` is the open handle OpenProcess just returned, which nothing else owns;
    // the `OwnedHandle` closes it once, on drop.
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// A handle borrowed for a Win32 call.
fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// The creation time of `process`, in 100 ns units since 1601: with the pid it names one
/// process, as a reused pid has a later creation time.
fn created(process: &OwnedHandle) -> Option<u64> {
    let mut times = [FILETIME::default(); 4];
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: `process` is a live handle with PROCESS_QUERY_LIMITED_INFORMATION, and the four
    // pointers are distinct live FILETIMEs the call writes.
    unsafe {
        GetProcessTimes(
            raw(process),
            &raw mut *creation,
            &raw mut *exit,
            &raw mut *kernel,
            &raw mut *user,
        )
    }
    .ok()?;
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

/// The executable of `process`; `None` when the call fails even with the longest path's room.
fn image(process: &OwnedHandle) -> Option<PathBuf> {
    for room in IMAGE_ROOMS {
        let mut buffer = vec![0_u16; room];
        let mut size = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `buffer` is a live buffer of `size` UTF-16 units; the call writes at most
        // that many, NUL included, and on success sets `size` to the units written before the
        // NUL. `process` is a live handle with PROCESS_QUERY_LIMITED_INFORMATION.
        let named = unsafe {
            QueryFullProcessImageNameW(
                raw(process),
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &raw mut size,
            )
        };
        if named.is_ok() {
            buffer.truncate(usize::try_from(size).ok()?.min(room));
            return Some(PathBuf::from(OsString::from_wide(&buffer)));
        }
    }
    None
}

/// Puts process `pid` in `tree`'s job. async-process gives no process handle, so one is opened by
/// pid: the caller's `Child` holds its own handle, so the pid cannot be reused meanwhile.
fn join(tree: &Tree, pid: u32) -> Result<(), Error> {
    // SAFETY: opening a process by id takes no pointer; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }
        .map_err(|error| os_error("could not open the agent's process", error))?;
    // SAFETY: `tree.job()` is the job handle `tree` owns, and `process` the live handle opened
    // above with the rights AssignProcessToJobObject needs.
    let joined = unsafe { AssignProcessToJobObject(tree.job(), process) };
    // SAFETY: `process` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(process) });
    joined.map_err(|error| os_error("could not put the agent in its job", error))
}

/// Resumes every thread of process `pid`, which started suspended. Std gives no thread handle, so
/// the threads are found with a Toolhelp snapshot. Fails unless at least one thread was suspended
/// and resumed: a thread whose suspend count was already 0 means `CREATE_SUSPENDED` was lost and
/// the agent ran before it joined its job. A running thread beside a suspended one is not that
/// (security software can inject one), so it does not fail the launch.
fn resume(pid: u32) -> Result<(), Error> {
    // Before the snapshot opens, so no early return leaves its handle open.
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(|source| Error::Os {
            what: "the thread entry struct is too large",
            source: io::Error::other(source),
        })?,
        ..THREADENTRY32::default()
    };
    // SAFETY: a thread snapshot of the whole system takes no pointer; its handle is closed below,
    // and nothing between here and there returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|error| os_error("could not list the agent's threads", error))?;
    let mut resumed = 0_usize;
    let mut running = 0_usize;
    // SAFETY: `snapshot` is the live snapshot handle and `entry` a THREADENTRY32 whose `dwSize`
    // is set, as Thread32First and Thread32Next require.
    let mut next = unsafe { Thread32First(snapshot, &raw mut entry) };
    while next.is_ok() {
        if entry.th32OwnerProcessID == pid {
            match resume_thread(entry.th32ThreadID) {
                Resume::Resumed => resumed += 1,
                Resume::WasRunning => running += 1,
                Resume::Failed => {}
            }
        }
        // SAFETY: as for Thread32First above.
        next = unsafe { Thread32Next(snapshot, &raw mut entry) };
    }
    // SAFETY: `snapshot` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(snapshot) });
    if resumed == 0 && running > 0 {
        return Err(Error::Os {
            what: "the agent did not start suspended",
            source: io::Error::other("its threads were already running"),
        });
    }
    if resumed == 0 {
        return Err(Error::Os {
            what: "could not resume the agent, which started suspended",
            source: io::Error::other("no thread of the agent resumed"),
        });
    }
    Ok(())
}

/// What [`resume_thread`] found.
enum Resume {
    /// The thread was suspended, and now runs.
    Resumed,
    /// The thread's suspend count was already 0: it was running.
    WasRunning,
    /// The thread could not be opened or resumed.
    Failed,
}

/// Resumes thread `id`. `ResumeThread` returns the previous suspend count, `u32::MAX` on error.
fn resume_thread(id: u32) -> Resume {
    // SAFETY: opening a thread by id takes no pointer; the handle is closed below.
    let Ok(thread) = (unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, id) }) else {
        return Resume::Failed;
    };
    // SAFETY: `thread` is a live handle opened with THREAD_SUSPEND_RESUME.
    let count = unsafe { ResumeThread(thread) };
    // SAFETY: `thread` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(thread) });
    match count {
        u32::MAX => Resume::Failed,
        0 => Resume::WasRunning,
        _ => Resume::Resumed,
    }
}

/// An [`Error::Os`] saying `what` failed, with the Win32 error.
fn os_error(what: &'static str, error: windows::core::Error) -> Error {
    Error::Os {
        what,
        source: io::Error::from(error),
    }
}
