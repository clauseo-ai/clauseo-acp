//! The helper: the test binary again, started by the owner as the test account (the guard's
//! role, `B_s`). It reads one line from the owner (the copied namespace handles, the package and
//! the paths), takes the handles with `from_inherited`, starts chrome-headless-shell in the
//! `LowBox` token inside a Job Object of its own, waits for the owner's `inspect`, reports what
//! it can see of chrome's processes, waits for chrome to exit and reports the exit codes. Each
//! report is a line starting [`MARK`], which the owner parses.
#![expect(
    unsafe_code,
    reason = "FFI to the Job Object, station and desktop name and wait calls"
)]

use std::ffi::c_void;
use std::fmt::Display;
use std::os::windows::io::OwnedHandle;
use std::path::Path;

use clauseo_lowbox::{
    LowBoxCommand, NamespaceHandles, PackageName, Stdio, package_sid, parse_handle_arg,
};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::StationsAndDesktops::{
    GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, TerminateProcess, WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::inspect;
use super::proxy::URL;
use super::win::{Res, failed, owned, raw, sleep_ms};

/// The start of every report line.
pub(crate) const MARK: &str = "LOWBOX|";
/// The longest the helper waits for chrome to exit after the inspection.
const CHROME_EXIT_MS: u32 = 90_000;

/// Prints one report line.
pub(crate) fn say(line: impl Display) {
    println!("{MARK}{line}");
}

/// What the owner tells the helper, as one line of `key=value` fields split by tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    /// The `--lowbox-handles` text of the handles copied into the helper.
    pub(crate) handles: String,
    /// The package name.
    pub(crate) package: String,
    /// chrome-headless-shell's full path.
    pub(crate) shell: String,
    /// `--user-data-dir`.
    pub(crate) profile: String,
    /// Where the screenshot goes.
    pub(crate) staging: String,
    /// chrome's `TEMP` and `TMP`.
    pub(crate) tmp: String,
    /// The proxy's port on 127.0.0.1.
    pub(crate) port: u16,
}

impl Config {
    /// The line the owner writes.
    pub(crate) fn line(&self) -> String {
        format!(
            "handles={}\tpackage={}\tshell={}\tprofile={}\tstaging={}\ttmp={}\tport={}",
            self.handles, self.package, self.shell, self.profile, self.staging, self.tmp, self.port
        )
    }

    /// The config in `line`.
    pub(crate) fn parse(line: &str) -> Res<Config> {
        let field = |key: &str| {
            line.trim_end()
                .split('\t')
                .find_map(|part| part.strip_prefix(key)?.strip_prefix('='))
                .map(str::to_owned)
                .ok_or(format!("the owner's line has no {key}"))
        };
        Ok(Config {
            handles: field("handles")?,
            package: field("package")?,
            shell: field("shell")?,
            profile: field("profile")?,
            staging: field("staging")?,
            tmp: field("tmp")?,
            port: field("port")?
                .parse()
                .map_err(|e| format!("the owner's port: {e}"))?,
        })
    }
}

/// The helper's whole run; a panic (the test's failure) reaches the owner as exit code 101.
pub(crate) fn run() {
    let mut input = std::io::stdin().lines();
    let first = input
        .next()
        .expect("the owner's line")
        .expect("the owner's line is text");
    let config = Config::parse(&first).unwrap();
    let package = PackageName::new(&config.package).unwrap();
    let values = parse_handle_arg(&config.handles).unwrap();
    let namespace = NamespaceHandles::from_inherited(&package, &values)
        .unwrap_or_else(|e| panic!("from_inherited: {e}"));
    say("handles taken=6");
    say(format_args!("station {}", named(station_name())));
    say(format_args!("desktop {}", named(desktop_name())));

    let shell = Path::new(&config.shell);
    let screenshot = Path::new(&config.staging).join("shot.png");
    let mut command = LowBoxCommand::new(shell);
    command
        .args([
            "--headless".to_owned(),
            "--remote-debugging-port=0".to_owned(),
            format!("--proxy-server=127.0.0.1:{}", config.port),
            format!("--user-data-dir={}", config.profile),
            format!("--screenshot={}", screenshot.display()),
            "--window-size=800,600".to_owned(),
            "--no-first-run".to_owned(),
            URL.to_owned(),
        ])
        .env("TEMP", &config.tmp)
        .env("TMP", &config.tmp)
        .current_dir(shell.parent().unwrap_or(shell))
        .stdin(Stdio::Null);
    let child = command
        .spawn_suspended(&namespace)
        .unwrap_or_else(|e| panic!("spawn_suspended: {e}"));
    let job = kill_on_close_job().unwrap();
    // SAFETY: an open job and the child's open process handle (all access).
    let assigned = unsafe { AssignProcessToJobObject(raw(&job), raw(&child.process)) };
    if let Err(e) = assigned {
        // Outside the job nothing would end the suspended child, so end it here.
        // SAFETY: the child's open process handle (all access).
        let ended = unsafe { TerminateProcess(raw(&child.process), 1) }.is_ok();
        panic!(
            "{} (the child was ended: {ended})",
            failed("AssignProcessToJobObject", &e)
        );
    }
    child.resume().unwrap_or_else(|e| panic!("resume: {e}"));
    say(format_args!("started pid={}", child.pid));
    say(format_args!(
        "machine {}",
        inspect::machine(raw(&child.process))
    ));

    let asked = input.any(|line| line.is_ok_and(|l| l.trim() == "inspect"));
    say(format_args!("inspect asked={}", u8::from(asked)));
    let seen = inspect::inspect(child.pid, &package_sid(&package).to_bytes()).unwrap();
    say(format_args!("inspected count={}", seen.len()));

    // SAFETY: the child's open process handle.
    let waited = unsafe { WaitForSingleObject(raw(&child.process), CHROME_EXIT_MS) };
    say(format_args!(
        "chrome exited={}",
        u8::from(waited == WAIT_OBJECT_0)
    ));
    report_exits(&seen);
    say("done");
    drop(job);
}

/// Prints each kept process's exit code, waiting up to 15 s for the stragglers.
fn report_exits(seen: &[inspect::Seen]) {
    for _ in 0..30 {
        let running = seen
            .iter()
            .filter_map(|s| s.handle.as_ref())
            .any(|h| inspect::exit_code(h).is_none());
        if !running {
            break;
        }
        sleep_ms(500);
    }
    for one in seen {
        let code = match &one.handle {
            None => "-".to_owned(),
            Some(h) => inspect::exit_code(h).map_or("running".to_owned(), |c| format!("{c:#x}")),
        };
        say(format_args!(
            "exit role={} pid={} code={code}",
            one.role, one.pid
        ));
    }
}

/// A Job Object that ends its processes when its last handle closes.
fn kill_on_close_job() -> Res<OwnedHandle> {
    // SAFETY: no attributes and no name; the handle is owned below.
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|e| failed("CreateJobObjectW", &e))?;
    let job = owned(job);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: an open job and a local limit struct of the class's size.
    unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(u32::MAX),
        )
    }
    .map_err(|e| failed("SetInformationJobObject", &e))?;
    Ok(job)
}

/// The name (`UOI_NAME`) of the station or desktop `object`.
fn object_name(object: HANDLE) -> Res<String> {
    let mut name = [0_u16; 256];
    let mut needed = 0_u32;
    // SAFETY: an open station or desktop handle and a local buffer of the size given.
    unsafe {
        GetUserObjectInformationW(
            object,
            UOI_NAME,
            Some(name.as_mut_ptr().cast::<c_void>()),
            u32::try_from(size_of_val(&name)).unwrap_or(u32::MAX),
            Some(&raw mut needed),
        )
    }
    .map_err(|e| failed("GetUserObjectInformationW(UOI_NAME)", &e))?;
    let len = name.iter().position(|&u| u == 0).unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..len]))
}

/// `name=<name>`, or `name=? <why>` when it could not be read.
fn named(name: Res<String>) -> String {
    name.map_or_else(|e| format!("name=? {e}"), |n| format!("name={n}"))
}

/// This process's window station's name.
fn station_name() -> Res<String> {
    // SAFETY: reads this process's station handle (not owned: never closed).
    let station =
        unsafe { GetProcessWindowStation() }.map_err(|e| failed("GetProcessWindowStation", &e))?;
    object_name(HANDLE(station.0))
}

/// This thread's desktop's name.
fn desktop_name() -> Res<String> {
    // SAFETY: reads this thread's desktop handle (not owned: never closed).
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }
        .map_err(|e| failed("GetThreadDesktop", &e))?;
    object_name(HANDLE(desktop.0))
}
