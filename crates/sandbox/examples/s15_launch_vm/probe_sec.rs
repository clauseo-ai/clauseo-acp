//! The security probes, which `probe.rs` hands on: what the agent tries against its runners, its
//! job, its inherited handles and the desktop. Each prints plain lines on stdout, parsed by
//! `checks_sec.rs`.
//!
//! - `runners`: `pid <self>`, `parent <pid>` (its inner runner, from
//!   `NtQueryInformationProcess(ProcessBasicInformation)`), then reads `<role> <pid>` lines from
//!   stdin up to `end` (the outer runner, the host and proxy stand-ins), `conhost <pid>` lines
//!   (each `conhost.exe` whose parent is the inner or the outer runner; a target, role
//!   `conhost-<pid>`) and `threads <role> <tid>...` lines (the thread ids of the inner and the
//!   outer runner), both from the harness's own Toolhelp snapshot (the agent's own is filtered).
//!   For each target:
//!   `runner-image <role> <pid> <path or code>`, one `runner <role> <right> <code>` per refused
//!   right ([`REFUSED`] and the token's [`TOKEN_RIGHTS`]; 0 when it opened), and one
//!   `runner-info <role> <right> <code>` per right that may be granted ([`INFO`]). Before
//!   those, as information from its own snapshot: `conhost-snapshot <parent role> <pid>` per
//!   `conhost.exe` under either runner, or `conhost-snapshot none` (or
//!   `conhost-snapshot error <code>`); for the inner and the outer runner
//!   `snapshot <role> <pid> <n>` (or `snapshot <role> <pid> error <code>`), what its own thread
//!   snapshot found (information);
//!   then per thread id the harness sent `thread <role> <tid>` and `<right> <code>` for each of
//!   [`THREAD_RIGHTS`] (0 when it opened).
//! - `breakaway`: starts `probe wait` suspended with `CREATE_BREAKAWAY_FROM_JOB`; prints
//!   `breakaway error <code>`, or `breakaway child <pid> in-job <true|false|error N>`, then waits
//!   for one stdin line (or its end) while the harness checks the child's job, then
//!   `breakaway killed <0 or code>` (the child is killed in every outcome).
//! - `handles`: `handles method <text>`, `handles count <n>`, `handles table <n>` (or
//!   `handles table error <why>`), then `handle <hex> pipe <name or ?> end <client|server|?>
//!   inherit <0|1> type <n>` per pipe handle, `handles pipes <n>`, and `handles done`.
//! - `desktop`: `ui-limits <hex>`, `desktop own <name>`, `desktop station <name>`,
//!   `desktop open-winsta0 <code>`, and when that opened `desktop set-winsta0 <code>` and (when
//!   that worked) `desktop open-default <code>` on `WinSta0`; `desktop switch <code>`,
//!   `desktop gdi <what it got>`, `desktop done`, then waits until killed, so the harness reads
//!   the window station's DACLs while the agent runs.

use std::io::BufRead as _;
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::FILE_TYPE_PIPE;
use windows::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_SUSPENDED, PROCESS_ALL_ACCESS, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_VM_READ, THREAD_ALL_ACCESS, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
};

use super::sys::{say, sleep, start};
use super::win_desk;
use super::win_sec::{self, OBJ_INHERIT};
use super::win_thread;

/// The process rights the agent must be refused on every target.
pub(crate) const REFUSED: [(&str, u32); 5] = [
    ("PROCESS_ALL_ACCESS", PROCESS_ALL_ACCESS.0),
    ("PROCESS_VM_READ", PROCESS_VM_READ.0),
    ("PROCESS_DUP_HANDLE", PROCESS_DUP_HANDLE.0),
    ("PROCESS_QUERY_INFORMATION", PROCESS_QUERY_INFORMATION.0),
    (
        "PROCESS_QUERY_LIMITED_INFORMATION",
        PROCESS_QUERY_LIMITED_INFORMATION.0,
    ),
];
/// The token rights the agent must be refused on both runners (`OpenProcessToken`).
pub(crate) const TOKEN_RIGHTS: [(&str, u32); 2] = [
    ("TOKEN_QUERY", TOKEN_QUERY.0),
    ("TOKEN_DUPLICATE", TOKEN_DUPLICATE.0),
];
/// The thread rights the agent must be refused on every thread of both runners.
pub(crate) const THREAD_RIGHTS: [(&str, u32); 3] = [
    ("THREAD_ALL_ACCESS", THREAD_ALL_ACCESS.0),
    (
        "THREAD_QUERY_LIMITED_INFORMATION",
        THREAD_QUERY_LIMITED_INFORMATION.0,
    ),
    ("THREAD_GET_CONTEXT", THREAD_GET_CONTEXT.0),
];
/// The right printed as information.
const INFO: [(&str, u32); 1] = [("SYNCHRONIZE", PROCESS_SYNCHRONIZE.0)];
/// How long one handle's name query may take.
const NAME_LIMIT: Duration = Duration::from_secs(2);
/// The prefix of a runner pipe's object name.
pub(crate) const PIPE_OBJECT_PREFIX: &str = r"\Device\NamedPipe\clauseo-run-";

/// Runs the security probe `what`; `None` when `what` isn't one of them.
pub(crate) fn run(what: &str) -> Option<ExitCode> {
    Some(match what {
        "runners" => runners(),
        "breakaway" => breakaway(),
        "handles" => handles(),
        "desktop" => desktop(),
        _ => return None,
    })
}

fn runners() -> ExitCode {
    say(&format!("pid {}", std::process::id()));
    let mut targets: Vec<(String, u32)> = Vec::new();
    match win_sec::parent_pid() {
        Ok(parent) => {
            say(&format!("parent {parent}"));
            targets.push(("inner".to_owned(), parent));
        }
        Err(why) => say(&format!("parent error {why}")),
    }
    // `<role> <pid>` lines (the outer runner, and for `lawyer-processes` the host and proxy
    // stand-ins), `conhost <pid>` and `threads <role> <tid>...` lines, then `end`.
    let mut given: Vec<(String, Vec<u32>)> = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line == "end" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("threads ") {
            let mut words = rest.split_whitespace();
            let role = words.next().unwrap_or_default().to_owned();
            match words
                .map(str::parse::<u32>)
                .collect::<Result<Vec<u32>, _>>()
            {
                Ok(tids) => given.push((role, tids)),
                Err(_) => say(&format!("target error {line:?}")),
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("conhost ") {
            match rest.trim().parse::<u32>() {
                Ok(pid) => targets.push((format!("conhost-{pid}"), pid)),
                Err(_) => say(&format!("target error {line:?}")),
            }
            continue;
        }
        match line
            .split_once(' ')
            .map(|(role, pid)| (role, pid.trim().parse::<u32>()))
        {
            Some((role, Ok(pid))) => targets.push((role.to_owned(), pid)),
            _ => say(&format!("target error {line:?}")),
        }
    }
    runner_threads(&targets, &given);
    for (role, pid) in &targets {
        match win_sec::image_of(*pid) {
            Ok(path) => say(&format!("runner-image {role} {pid} {path}")),
            Err(code) => say(&format!("runner-image {role} {pid} error {code}")),
        }
        for (right, access) in REFUSED {
            say(&format!(
                "runner {role} {right} {}",
                win_sec::open_code(*pid, access)
            ));
        }
        for (right, access) in TOKEN_RIGHTS {
            match win_sec::token_code(*pid, access) {
                Ok(code) => say(&format!("runner {role} {right} {code}")),
                // The token can't be reached when the process can't be opened at all.
                Err(code) => say(&format!(
                    "runner {role} {right} {code} (the PROCESS_QUERY_LIMITED_INFORMATION open \
                     failed)"
                )),
            }
        }
        for (right, access) in INFO {
            say(&format!(
                "runner-info {role} {right} {}",
                win_sec::open_code(*pid, access)
            ));
        }
    }
    say("runners done");
    ExitCode::SUCCESS
}

/// The console hosts and the threads of the two runners (the targets with roles `inner` and
/// `outer`): what this process's own snapshot finds of both is printed as information; the
/// threads tried are the ids in `given` (from the harness).
fn runner_threads(targets: &[(String, u32)], given: &[(String, Vec<u32>)]) {
    let runners: Vec<(String, u32)> = targets
        .iter()
        .filter(|(role, _)| role == "inner" || role == "outer")
        .cloned()
        .collect();
    let pids: Vec<u32> = runners.iter().map(|(_, pid)| *pid).collect();
    match win_thread::children_named(&pids, "conhost.exe") {
        Ok(found) if found.is_empty() => say("conhost-snapshot none"),
        Ok(found) => {
            for (parent, pid) in found {
                let role = runners
                    .iter()
                    .find(|(_, p)| *p == parent)
                    .map_or("?", |(role, _)| role.as_str());
                say(&format!("conhost-snapshot {role} {pid}"));
            }
        }
        Err(code) => say(&format!("conhost-snapshot error {code}")),
    }
    for (role, pid) in &runners {
        match win_thread::threads_of(*pid) {
            Ok(threads) => say(&format!("snapshot {role} {pid} {}", threads.len())),
            Err(code) => say(&format!("snapshot {role} {pid} error {code}")),
        }
    }
    for (role, tids) in given {
        for &tid in tids {
            let codes: Vec<String> = THREAD_RIGHTS
                .iter()
                .map(|(right, access)| {
                    format!("{right} {}", win_thread::open_thread_code(tid, *access))
                })
                .collect();
            say(&format!("thread {role} {tid} {}", codes.join(" ")));
        }
    }
}

fn breakaway() -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            say(&format!("breakaway exe-error {error}"));
            return ExitCode::from(1);
        }
    };
    let mut command = Command::new(exe);
    command
        .args(["probe", "wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB.0 | CREATE_SUSPENDED.0);
    match start(&mut command) {
        Err(error) => {
            let code = error
                .raw_os_error()
                .map_or_else(|| error.kind().to_string(), |c| c.to_string());
            say(&format!("breakaway error {code}"));
        }
        Ok(mut child) => {
            let in_job = match win_sec::in_any_job(HANDLE(child.as_raw_handle())) {
                Ok(inside) => inside.to_string(),
                Err(code) => format!("error {code}"),
            };
            say(&format!("breakaway child {} in-job {in_job}", child.id()));
            // The harness checks the child is in the runner's own job while it lives.
            drop(std::io::stdin().lock().lines().next());
            let killed = match child.kill() {
                Ok(()) => "0".to_owned(),
                Err(error) => error
                    .raw_os_error()
                    .map_or_else(|| error.kind().to_string(), |c| c.to_string()),
            };
            drop(child.wait());
            say(&format!("breakaway killed {killed}"));
        }
    }
    ExitCode::SUCCESS
}

fn handles() -> ExitCode {
    // The table is read first, before this probe opens anything of its own.
    let count = win_sec::handle_count();
    let table = win_sec::own_handles();
    say(
        "handles method the handle table from NtQueryInformationProcess(ProcessHandleInformation) \
         on this process; for each handle GetFileType, and for pipes GetNamedPipeInfo's end and \
         the name from NtQueryObject(ObjectNameInformation) on a thread with a 2 s limit",
    );
    match count {
        Ok(count) => say(&format!("handles count {count}")),
        Err(code) => say(&format!("handles count error {code}")),
    }
    let entries = match table {
        Ok(entries) => {
            say(&format!("handles table {}", entries.len()));
            entries
        }
        Err(why) => {
            say(&format!("handles table error {why}"));
            return ExitCode::SUCCESS;
        }
    };
    let mut pipes = 0usize;
    for entry in entries {
        if win_sec::file_type(entry.value) != FILE_TYPE_PIPE.0 {
            continue;
        }
        pipes += 1;
        let name = win_sec::object_name(entry.value, NAME_LIMIT)
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "?".to_owned());
        let end = match win_sec::pipe_server_end(entry.value) {
            Ok(true) => "server",
            Ok(false) => "client",
            Err(_) => "?",
        };
        let inherit = u8::from(entry.attributes & OBJ_INHERIT != 0);
        say(&format!(
            "handle {:#x} pipe {name} end {end} inherit {inherit} type {} access {:#x}",
            entry.value, entry.type_index, entry.granted
        ));
    }
    say(&format!("handles pipes {pipes}"));
    say("handles done");
    ExitCode::SUCCESS
}

fn desktop() -> ! {
    match super::win::job_ui_limits() {
        Ok(limits) => say(&format!("ui-limits {limits:#x}")),
        Err(error) => say(&format!("ui-limits error {error}")),
    }
    match win_desk::own_desktop_name() {
        Ok(name) => say(&format!("desktop own {name}")),
        Err(code) => say(&format!("desktop own error {code}")),
    }
    match win_desk::own_station_name() {
        Ok(name) => say(&format!("desktop station {name}")),
        Err(code) => say(&format!("desktop station error {code}")),
    }
    let winsta0 = win_desk::try_winsta0();
    say(&format!("desktop open-winsta0 {}", winsta0.open));
    if let Some(code) = winsta0.set {
        say(&format!("desktop set-winsta0 {code}"));
    }
    if let Some(code) = winsta0.desktop {
        say(&format!("desktop open-default {code}"));
    }
    say(&format!("desktop switch {}", win_desk::switch_own_code()));
    match win_desk::capture() {
        Ok(got) => say(&format!(
            "desktop gdi {}x{} bitblt {} bytes {} nonzero {}",
            got.width, got.height, got.blit, got.bytes, got.nonzero
        )),
        Err(why) => say(&format!("desktop gdi error {why}")),
    }
    say("desktop done");
    loop {
        sleep(Duration::from_secs(3600));
    }
}
