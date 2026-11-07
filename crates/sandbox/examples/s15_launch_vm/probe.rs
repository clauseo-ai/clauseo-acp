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
//! - `job-ui [atom]`: `ui-limits <hex>`, the UI limits of the innermost job it runs in, and
//!   `ui-effect station <name>`, its own window station; then the limits' effects:
//!   `ui-effect desktop-create <code>` (`CreateDesktopW` on its own window station, 0 when it
//!   made one, closed at once), `ui-effect switch-desktop <code>` (`SwitchDesktop` to a desktop
//!   it made, then straight back, `win_switch.rs`; `create-failed <code>` when the create
//!   failed; `ui-effect switch-desktop-back <code>` when it switched) and `ui-effect
//!   clipboard-write ok` or `ui-effect clipboard-write failed <call> <code>` (the call that
//!   failed, `GetLastError`); with `atom`, `ui-info atom <name> <n>` (`GlobalFindAtomW`, 0 when
//!   none). `token` prints all but the atom line too.
//! - `ui-control [sa]`: the harness's positive control, started under one of the control
//!   tokens on the control station's `Default` desktop, outside the runner's job: `ui-control
//!   station <name>`, `ui-control user <sid>`, `ui-control integrity <sid>`, `ui-control
//!   restricted <n>`, `ui-control ui-limits <hex>` (information), `ui-control desktop-sa
//!   default|explicit`, then `ui-control desktop-create <code>`, `ui-control switch-desktop
//!   <code>` and `ui-control clipboard-write ok` or `ui-control clipboard-write failed <call>
//!   <code>`, the same calls as `job-ui`'s effects. With `sa`, `CreateDesktopW` gets explicit
//!   `SECURITY_ATTRIBUTES` whose DACL names the token's user and `SYSTEM`.
//! - `ui-relay <desktop> <desktop user> <ms>`: the control's variants (a) and (g), started by
//!   the harness as a pool user on a control station (`<desktop>` is that station's for (a), the
//!   agent's own `ClauseoRun-<hex>\Default` for (g)): makes the agent's token from its own as
//!   the inner runner does and starts `probe ui-control` with it on `<desktop>`, waiting up to
//!   `<ms>`; prints the child's lines, then `ui-relay exit <code or None>`, or `ui-relay error
//!   <why>`.
//! - `clipboard-read`: `clipboard-read ok <text>` or `clipboard-read error <why>`.
//! - `clipboard-write <text>`: `clipboard-write ok` or `clipboard-write error <why>`.
//! - `find-atom <name>`: `atom <n>`, 0 when `GlobalFindAtomW` finds none.
//! - `read <path>`: `read ok <bytes>` or `read error <code>`.
//! - `write <path>`: `write <code>` from opening it for writing (nothing is written).
//! - `runners`, `breakaway`, `handles`, `desktop`: the security probes, `probe_sec.rs`.

use std::io::BufRead as _;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use clauseo_sandbox::windows::sids::Sid;
use clauseo_sandbox::windows::token::default_dacl;

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
            job_ui(rest.first().map(String::as_str));
            ExitCode::SUCCESS
        }
        "ui-control" => {
            ui_control(rest.first().is_some_and(|a| a == "sa"));
            ExitCode::SUCCESS
        }
        "ui-relay" => match rest {
            [desktop, user, ms] => {
                ui_relay(desktop, user, ms);
                ExitCode::SUCCESS
            }
            _ => usage("ui-relay <desktop> <desktop user> <ms>"),
        },
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
             ui-control|ui-relay|clipboard-read|clipboard-write|find-atom|runners|breakaway|\
             handles|desktop",
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

/// The UI limits of the innermost job it is in, and their effect. A process node starts sits in
/// libuv's own job inside the runner's, whose limits read as none; the runner's still bind it
/// (nested jobs' limits combine), and the effect shows they do: `CreateDesktopW` on its own
/// station (whose DACL grants the pool user `GA`) refused with 5 by the `DESKTOP` limit, and a
/// clipboard write refused with 5 by `WRITECLIPBOARD`. `atom`, the harness's global atom, is
/// looked up as information: atoms are per window station, so it is hidden here anyway.
fn job_ui(atom: Option<&str>) {
    match win::job_ui_limits() {
        Ok(limits) => say(&format!("ui-limits {limits:#x}")),
        Err(error) => say(&format!("ui-limits error {}", code(&error))),
    }
    match super::win_desk::own_station_name() {
        Ok(name) => say(&format!("ui-effect station {name}")),
        Err(code) => say(&format!("ui-effect station error {code}")),
    }
    say(&format!(
        "ui-effect desktop-create {}",
        super::win_desk::create_desktop_code()
    ));
    say(&format!(
        "ui-effect desktop-create-default-heap {}",
        super::win_desk::create_desktop_default_heap_code()
    ));
    switch("ui-effect");
    match ui::write_failed("clauseo-ui-effect") {
        Ok(()) => say("ui-effect clipboard-write ok"),
        Err(failed) => say(&format!(
            "ui-effect clipboard-write failed {} {}",
            failed.call, failed.code
        )),
    }
    if let Some(name) = atom {
        say(&format!("ui-info atom {name} {}", ui::find_atom(name)));
    }
}

/// `SwitchDesktop` to a new desktop and back (`win_switch.rs`), as `<prefix> switch-desktop
/// <code>` and, when it switched, `<prefix> switch-desktop-back <code>`.
fn switch(prefix: &str) {
    let switched = super::win_switch::switch_desktop();
    say(&format!("{prefix} switch-desktop {}", switched.code));
    if let Some(back) = switched.back {
        say(&format!("{prefix} switch-desktop-back {back}"));
    }
}

/// The positive control's calls, as `job_ui` makes them, and what tells its token and
/// station apart (information for the harness, which checks the shape). With `sa`,
/// `CreateDesktopW` gets a DACL of the token's user and `SYSTEM`.
fn ui_control(sa: bool) {
    match super::win_desk::own_station_name() {
        Ok(name) => say(&format!("ui-control station {name}")),
        Err(code) => say(&format!("ui-control station error {code}")),
    }
    let mut user = None;
    match win::own_token() {
        Ok(facts) => {
            say(&format!("ui-control user {}", facts.user));
            say(&format!("ui-control integrity {}", facts.integrity));
            say(&format!("ui-control restricted {}", facts.restricted.len()));
            user = Some(facts.user);
        }
        Err(error) => say(&format!("ui-control token error {}", code(&error))),
    }
    match win::job_ui_limits() {
        Ok(limits) => say(&format!("ui-control ui-limits {limits:#x}")),
        Err(error) => say(&format!("ui-control ui-limits error {}", code(&error))),
    }
    let created = match (sa, user) {
        (false, _) => {
            say("ui-control desktop-sa default");
            super::win_desk::create_desktop_code().to_string()
        }
        (true, Some(user)) => {
            say("ui-control desktop-sa explicit");
            let sddl = format!("D:(A;;GA;;;{user})(A;;GA;;;SY)");
            super::win_desk::create_desktop_code_sa(&sddl)
                .map_or_else(|c| format!("sddl-error {c}"), |c| c.to_string())
        }
        (true, None) => "no-user".to_owned(),
    };
    say(&format!("ui-control desktop-create {created}"));
    say(&format!(
        "ui-control desktop-create-default-heap {}",
        super::win_desk::create_desktop_default_heap_code()
    ));
    switch("ui-control");
    match ui::write_failed("clauseo-ui-control") {
        Ok(()) => say("ui-control clipboard-write ok"),
        Err(failed) => say(&format!(
            "ui-control clipboard-write failed {} {}",
            failed.call, failed.code
        )),
    }
}

/// Run as the pool user: the agent's token made from its own (the inner runner's recipe, with
/// the agent's default DACL for `desktop_user`), and `probe ui-control` under it on `desktop`.
fn ui_relay(desktop: &str, desktop_user: &str, ms: &str) {
    let made = (|| {
        let own = win::own_token().map_err(|e| format!("own token: {}", code(&e)))?;
        let pool = Sid::parse(&own.user).map_err(|e| e.to_string())?;
        let lawyer = Sid::parse(desktop_user).map_err(|e| e.to_string())?;
        let dacl = default_dacl(&pool, &lawyer);
        let (token, _) = super::win_ctl::shaped_token(super::win_ctl::Shape::Agent, Some(&dacl))?;
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", code(&e)))?;
        let cwd = std::env::current_dir().map_err(|e| format!("current_dir: {}", code(&e)))?;
        let limit = Duration::from_millis(ms.parse().unwrap_or(10_000));
        super::win_ctl::run_as(&token, desktop, &exe, &["probe", "ui-control"], &cwd, limit)
    })();
    match made {
        Ok(ran) => {
            for line in &ran.lines {
                say(line);
            }
            say(&format!("ui-relay exit {:?}", ran.code));
        }
        Err(why) => say(&format!("ui-relay error {why}")),
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
            job_ui(None);
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
