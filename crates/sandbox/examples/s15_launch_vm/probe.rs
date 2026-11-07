//! `s15_launch_vm probe <what> ...`: the programs the runner starts inside the sandbox. Each prints
//! plain `key value` lines on stdout, which the rows parse.
//!
//! - `token`: `user`, `integrity`, one `restricted` per restricting SID, one `group <sid> <hex
//!   attributes>` per group, one `privilege <name> <hex attributes>` per privilege.
//! - `wait`: `pid <n>`, then sleeps until killed.
//! - `open-process <pid>`: `open-process <pid> <code>` (0 when it opened).
//! - `open-pipes <name>...`: `pipe <name> read <code> write <code>` per name.
//! - `spawn-child`: starts `probe wait`, prints `pid <self>` and `child <pid>`, then waits.
//! - `echo`: `echo <line>` for each stdin line; exits 0 at end of input.
//! - `exit <n>`: exits `n` (decimal, or hex with `0x`; any 32-bit code).
//! - `job-ui`: `ui-limits <hex>`, the UI limits of the job it runs in (`token` prints it too).
//! - `clipboard-read`: `clipboard-read ok <text>` or `clipboard-read error <why>`.
//! - `clipboard-write <text>`: `clipboard-write ok` or `clipboard-write error <why>`.
//! - `find-atom <name>`: `atom <n>`, 0 when `GlobalFindAtomW` finds none.
//! - `read <path>`: `read ok <bytes>` or `read error <code>`.
//! - `write <path>`: `write <code>` from opening it for writing (nothing is written).
//! - `runners`, `breakaway`, `handles`, `desktop`: the security probes, `probe_sec.rs`.

use std::io::BufRead as _;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use super::sys::{say, sleep, start};
use super::{ui, win};

/// Runs the probe `args` names (the arguments after `probe`).
pub(crate) fn run(args: &[String]) -> ExitCode {
    let what = args.first().map_or("", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    if let Some(code) = super::probe_sec::run(what) {
        return code;
    }
    match what {
        "token" => token(),
        "wait" => {
            say(&format!("pid {}", std::process::id()));
            wait_forever()
        }
        "open-process" => match rest.first().and_then(|pid| pid.parse::<u32>().ok()) {
            Some(pid) => {
                say(&format!(
                    "open-process {pid} {}",
                    win::open_process_code(pid)
                ));
                ExitCode::SUCCESS
            }
            None => usage("open-process <pid>"),
        },
        "open-pipes" => {
            for name in rest {
                let path = format!(r"\\.\pipe\{name}");
                let read = win::open_file_code(&path, false);
                let write = win::open_file_code(&path, true);
                say(&format!("pipe {name} read {read} write {write}"));
            }
            ExitCode::SUCCESS
        }
        "spawn-child" => spawn_child(),
        "echo" => echo(),
        "exit" => match rest.first().and_then(|n| exit_code(n)) {
            Some(n) => match u8::try_from(n) {
                Ok(small) => ExitCode::from(small),
                Err(_) => exit_with(n),
            },
            None => usage("exit <n>"),
        },
        "job-ui" => {
            job_ui();
            ExitCode::SUCCESS
        }
        "clipboard-read" => {
            match ui::read_text() {
                Ok(text) => say(&format!("clipboard-read ok {text}")),
                Err(why) => say(&format!("clipboard-read error {why}")),
            }
            ExitCode::SUCCESS
        }
        "clipboard-write" => match rest.first() {
            Some(text) => {
                match ui::write_text(text) {
                    Ok(()) => say("clipboard-write ok"),
                    Err(why) => say(&format!("clipboard-write error {why}")),
                }
                ExitCode::SUCCESS
            }
            None => usage("clipboard-write <text>"),
        },
        "find-atom" => match rest.first() {
            Some(name) => {
                say(&format!("atom {}", ui::find_atom(name)));
                ExitCode::SUCCESS
            }
            None => usage("find-atom <name>"),
        },
        "read" => match rest.first() {
            Some(path) => {
                match std::fs::read(path) {
                    Ok(bytes) => say(&format!("read ok {}", bytes.len())),
                    Err(error) => say(&format!("read error {}", code(&error))),
                }
                ExitCode::SUCCESS
            }
            None => usage("read <path>"),
        },
        "write" => match rest.first() {
            Some(path) => {
                say(&format!("write {}", win::open_file_code(path, true)));
                ExitCode::SUCCESS
            }
            None => usage("write <path>"),
        },
        _ => usage(
            "token|wait|open-process|open-pipes|spawn-child|echo|exit|read|write|job-ui|\
             clipboard-read|clipboard-write|find-atom|runners|breakaway|handles|desktop",
        ),
    }
}

/// `text` as an exit code: decimal, or hex after `0x`.
fn exit_code(text: &str) -> Option<u32> {
    match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Exits with a code above 255, which `ExitCode` can't carry.
#[expect(
    clippy::exit,
    reason = "the probe's 32-bit exit code, for the AgentReserved row"
)]
fn exit_with(code: u32) -> ExitCode {
    std::process::exit(i32::from_ne_bytes(code.to_ne_bytes()))
}

/// The UI limits of the innermost job it is in, and their effect: a clipboard write, which the
/// runner's job refuses. A process node starts sits in libuv's own job inside the runner's,
/// whose limits read as none; the runner's still bind it (nested jobs' limits combine), and the
/// write's refusal shows they do.
fn job_ui() {
    match win::job_ui_limits() {
        Ok(limits) => say(&format!("ui-limits {limits:#x}")),
        Err(error) => say(&format!("ui-limits error {}", code(&error))),
    }
    match ui::write_text("clauseo-ui-effect") {
        Ok(()) => say("ui-effect clipboard-write ok"),
        Err(why) => say(&format!("ui-effect clipboard-write refused {why}")),
    }
}

fn token() -> ExitCode {
    match win::own_token() {
        Ok(facts) => {
            say(&format!("user {}", facts.user));
            say(&format!("integrity {}", facts.integrity));
            for sid in &facts.restricted {
                say(&format!("restricted {sid}"));
            }
            for (sid, attributes) in &facts.groups {
                say(&format!("group {sid} {attributes:#x}"));
            }
            for (name, attributes) in &facts.privileges {
                say(&format!("privilege {name} {attributes:#x}"));
            }
            job_ui();
            ExitCode::SUCCESS
        }
        Err(error) => {
            say(&format!("token error {}", code(&error)));
            ExitCode::from(1)
        }
    }
}

fn spawn_child() -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            say(&format!("spawn error {}", code(&error)));
            return ExitCode::from(1);
        }
    };
    let mut command = Command::new(exe);
    command
        .args(["probe", "wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match start(&mut command) {
        Ok(mut child) => {
            say(&format!("pid {}", std::process::id()));
            say(&format!("child {}", child.id()));
            drop(child.wait());
            ExitCode::SUCCESS
        }
        Err(error) => {
            say(&format!("spawn error {}", code(&error)));
            ExitCode::from(1)
        }
    }
}

fn echo() -> ExitCode {
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) => say(&format!("echo {line}")),
            Err(_) => return ExitCode::from(1),
        }
    }
    ExitCode::SUCCESS
}

fn wait_forever() -> ! {
    loop {
        sleep(Duration::from_secs(3600));
    }
}

fn usage(what: &str) -> ExitCode {
    say(&format!("usage: s15_launch_vm probe {what}"));
    ExitCode::from(2)
}

fn code(error: &std::io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string())
}
