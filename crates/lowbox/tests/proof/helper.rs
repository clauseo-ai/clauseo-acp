//! The helper: the test binary again, started by the owner as the test account (the guard's
//! role, `B_s`). It reads one line from the owner (the copied namespace handles, the package and
//! the paths), takes the handles with `from_inherited`, starts chrome-headless-shell in the
//! `LowBox` token inside a Job Object of its own, waits for the owner's `inspect`, reports what
//! it can see of chrome's processes, waits for chrome to exit and reports the exit codes. Each
//! report is a line starting [`MARK`], which the owner parses. Chrome's standard output is
//! `NUL` and its standard error goes to [`STDERR_LOG`] in staging, so neither reaches the
//! report pipe.
//!
//! The helper drives chrome over CDP on two anonymous pipes (`cdp.rs`), never a TCP port:
//! `--remote-debugging-pipe` with `--remote-debugging-io-pipes=<chrome reads>,<chrome writes>`,
//! the handle values in decimal, both passed with `inherit_handle`. Chromium 157 needs
//! `--remote-debugging-pipe` for the handler to start at all; `--remote-debugging-io-pipes`
//! alone only names the handles. The sequence: `Browser.getVersion`, `Target.createTarget`
//! (`about:blank`), `Target.attachToTarget` (flat), `Page.enable`, `Page.navigate` to the
//! proxy's page (whose answer the proxy holds while the helper inspects), `Page.loadEventFired`,
//! `Page.captureScreenshot` into `staging\shot.png`, `Browser.close`. Twice, after the
//! inspection and before `Browser.close`, it counts the TCP listeners of the job's processes
//! (`listen.rs`).
#![expect(
    unsafe_code,
    reason = "FFI to the Job Object, station and desktop name and wait calls"
)]

use std::ffi::c_void;
use std::fmt::Display;
use std::fs::File;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use clauseo_lowbox::{
    LowBoxCommand, NamespaceHandles, PackageName, Spawned, Stdio, package_sid, parse_handle_arg,
};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseWindowStation, GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW,
    OpenWindowStationW, UOI_NAME,
};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, TerminateProcess, WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::cdp::{Cdp, Reply, failure, field};
use super::inspect;
use super::listen;
use super::proxy::URL;
use super::win::{Res, code, failed, owned, raw, sleep_ms, wide};
use super::write;

/// The start of every report line.
pub(crate) const MARK: &str = "LOWBOX|";
/// The file in staging that gets chrome's standard error.
pub(crate) const STDERR_LOG: &str = "chrome-stderr.log";
/// The longest the helper waits for chrome to exit after `Browser.close`.
pub(crate) const CHROME_EXIT_MS: u32 = 30_000;
/// The longest each CDP step waits for its reply or event.
const STEP: Duration = Duration::from_secs(20);
/// The longest `Browser.close` waits for its reply.
const CLOSE_STEP: Duration = Duration::from_secs(10);
/// The longest it waits for chrome's standard error to close after the job ends.
const STDERR_WAIT: Duration = Duration::from_secs(10);
/// `READ_CONTROL`: enough to open the station by name.
const READ_CONTROL: u32 = 0x0002_0000;

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
    say(format_args!("station {}", station_report()));
    say(format_args!("desktop {}", named(desktop_name())));

    let shell = Path::new(&config.shell);
    let staging = Path::new(&config.staging);
    // Commands go in on one pipe, replies and events come back on the other.
    let (chrome_reads, helper_writes) = std::io::pipe().expect("the CDP command pipe");
    let (helper_reads, chrome_writes) = std::io::pipe().expect("the CDP reply pipe");
    let (r, w) = (chrome_reads.as_raw_handle(), chrome_writes.as_raw_handle());
    let mut command = LowBoxCommand::new(shell);
    command
        .args([
            "--headless".to_owned(),
            "--remote-debugging-pipe".to_owned(),
            format!("--remote-debugging-io-pipes={},{}", r as usize, w as usize),
            format!("--proxy-server=127.0.0.1:{}", config.port),
            format!("--user-data-dir={}", config.profile),
            "--window-size=800,600".to_owned(),
            "--no-first-run".to_owned(),
        ])
        // The child's environment is only what is set here (nothing from the helper): the
        // system folders chrome and Winsock need, and its own temp folder.
        .env("SystemRoot", system_var("SystemRoot", r"C:\Windows"))
        .env("windir", system_var("windir", r"C:\Windows"))
        .env("SystemDrive", system_var("SystemDrive", "C:"))
        .env("TEMP", &config.tmp)
        .env("TMP", &config.tmp)
        .current_dir(shell.parent().unwrap_or(shell))
        .stdin(Stdio::Null)
        .stdout(Stdio::Null)
        .stderr(Stdio::Piped)
        .inherit_handle(r)
        .inherit_handle(w);
    // The job comes first: once chrome exists, every failure before the resume ends it.
    let job = kill_on_close_job().unwrap();
    let mut child = command
        .spawn_suspended(&namespace)
        .unwrap_or_else(|e| panic!("spawn_suspended: {e}"));
    // Chrome has its own copies now; the helper keeps only its ends, so a dead chrome closes them.
    drop((chrome_reads, chrome_writes));
    // SAFETY: an open job and the child's open process handle (all access).
    let assigned = unsafe { AssignProcessToJobObject(raw(&job), raw(&child.process)) };
    if let Err(e) = assigned {
        end_and_panic(&child, &failed("AssignProcessToJobObject", &e));
    }
    if let Err(e) = child.resume() {
        end_and_panic(&child, &format!("resume: {e}"));
    }
    let stderr = child
        .stderr
        .take()
        .map(|pipe| drain(pipe, &staging.join(STDERR_LOG)));
    say(format_args!("started pid={}", child.pid));
    say(format_args!(
        "machine {}",
        inspect::machine(raw(&child.process))
    ));

    let mut cdp = Cdp::start(helper_writes, helper_reads);
    let (session, navigate) = open_page(&mut cdp).unwrap_or_else(|e| panic!("{e}"));

    let asked = input.any(|line| line.is_ok_and(|l| l.trim() == "inspect"));
    say(format_args!("inspect asked={}", u8::from(asked)));
    let seen = inspect::inspect(child.pid, &package_sid(&package).to_bytes()).unwrap();
    let inspected: Vec<u32> = seen.iter().map(|s| s.pid).collect();
    listen::report("inspect", &job, &inspected);
    say(format_args!("inspected count={}", seen.len()));

    finish_page(&mut cdp, &session, navigate, staging).unwrap_or_else(|e| panic!("{e}"));
    listen::report("close", &job, &inspected);
    close_browser(&mut cdp).unwrap_or_else(|e| panic!("{e}"));

    // SAFETY: the child's open process handle.
    let waited = unsafe { WaitForSingleObject(raw(&child.process), CHROME_EXIT_MS) };
    let chrome_code = inspect::exit_code(&child.process).map_or("-".to_owned(), hex);
    say(format_args!(
        "chrome exited={} code={chrome_code}",
        u8::from(waited == WAIT_OBJECT_0)
    ));
    report_exits(&seen);
    drop(job);
    let stderr = stderr.map_or("pipe=none".to_owned(), |done| {
        done.recv_timeout(STDERR_WAIT)
            .unwrap_or_else(|_| "pipe=open-after-10s".to_owned())
    });
    say(format_args!("stderr {stderr}"));
    say("done");
}

/// Up to the held page: `Browser.getVersion`, a new `about:blank` target, a flat session on
/// it, `Page.enable`, and `Page.navigate` sent without waiting (its reply needs the page the
/// proxy holds). The session id and the navigate's command id.
fn open_page(cdp: &mut Cdp) -> Res<(String, u64)> {
    let version = cdp.call("Browser.getVersion", json!({}), None, STEP)?;
    say(format_args!(
        "cdp method=Browser.getVersion ok product={} protocolVersion={}",
        field(&version, "product"),
        field(&version, "protocolVersion")
    ));
    let target = cdp.call(
        "Target.createTarget",
        json!({ "url": "about:blank" }),
        None,
        STEP,
    )?;
    let target_id = target
        .get("targetId")
        .and_then(Value::as_str)
        .ok_or_else(|| failure("Target.createTarget", "no targetId"))?
        .to_owned();
    say("cdp method=Target.createTarget ok");
    let attached = cdp.call(
        "Target.attachToTarget",
        json!({ "targetId": target_id, "flatten": true }),
        None,
        STEP,
    )?;
    let session = attached
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| failure("Target.attachToTarget", "no sessionId"))?
        .to_owned();
    say("cdp method=Target.attachToTarget ok flatten=1");
    cdp.call("Page.enable", json!({}), Some(&session), STEP)?;
    say("cdp method=Page.enable ok");
    let navigate = cdp.send("Page.navigate", json!({ "url": URL }), Some(&session))?;
    say(format_args!("cdp method=Page.navigate sent url={URL}"));
    Ok((session, navigate))
}

/// After the release: the navigate's reply, `Page.loadEventFired`, and the screenshot, decoded
/// into `staging\shot.png`.
fn finish_page(cdp: &mut Cdp, session: &str, navigate: u64, staging: &Path) -> Res<()> {
    let navigated = match cdp.reply(navigate, "Page.navigate", STEP)? {
        Reply::Result(result) => result,
        Reply::Closed => return Err(failure("Page.navigate", "chrome's pipe closed")),
    };
    if let Some(error) = navigated.get("errorText").and_then(Value::as_str) {
        return Err(failure("Page.navigate", error));
    }
    say("cdp method=Page.navigate ok");
    cdp.event("Page.loadEventFired", session, STEP)?;
    say("cdp event=Page.loadEventFired ok");
    let shot = cdp.call(
        "Page.captureScreenshot",
        json!({ "format": "png" }),
        Some(session),
        STEP,
    )?;
    let data = shot
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| failure("Page.captureScreenshot", "no data"))?;
    let png = BASE64
        .decode(data)
        .map_err(|e| failure("Page.captureScreenshot", &format!("base64: {e}")))?;
    write::write(&staging.join("shot.png"), &png)
        .map_err(|e| failure("Page.captureScreenshot", &format!("writing shot.png: {e}")))?;
    say(format_args!(
        "cdp method=Page.captureScreenshot ok bytes={}",
        png.len()
    ));
    Ok(())
}

/// `Browser.close`: its reply, or chrome's pipe closing first, within [`CLOSE_STEP`].
fn close_browser(cdp: &mut Cdp) -> Res<()> {
    let id = cdp.send("Browser.close", json!({}), None)?;
    let how = match cdp.reply(id, "Browser.close", CLOSE_STEP)? {
        Reply::Result(_) => "reply",
        Reply::Closed => "pipe-closed",
    };
    say(format_args!("cdp method=Browser.close ok by={how}"));
    Ok(())
}

/// `code` in hex.
fn hex(code: u32) -> String {
    format!("{code:#x}")
}

/// Ends the suspended `child` (nothing else would: it may be outside the job), then fails.
fn end_and_panic(child: &Spawned, why: &str) -> ! {
    // SAFETY: the child's open process handle (all access).
    let ended = unsafe { TerminateProcess(raw(&child.process), 1) }.is_ok();
    panic!("{why} (the child was ended: {ended})");
}

/// Copies chrome's standard error into `path` on a thread of its own; the receiver gets
/// `bytes=<n>` or `error=<why>` once the pipe closes.
fn drain(pipe: File, path: &Path) -> Receiver<String> {
    let (done, outcome) = mpsc::channel();
    let path = path.to_owned();
    thread::spawn(move || {
        let mut pipe = pipe;
        let copied = write::create(&path).and_then(|mut file| std::io::copy(&mut pipe, &mut file));
        let text = match copied {
            Ok(bytes) => format!("bytes={bytes}"),
            Err(e) => format!("error={}", e.to_string().replace(' ', "_")),
        };
        let _owner_gone = done.send(text);
    });
    outcome
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
            Some(h) => inspect::exit_code(h).map_or("running".to_owned(), hex),
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

/// `name=<name> opens=<1 or err…>`: this process's station's name, and whether the station of
/// that name opens by name from here (`READ_CONTROL`).
fn station_report() -> String {
    let name = match station_name() {
        Ok(name) => name,
        Err(e) => return format!("name=? opens=- {e}"),
    };
    let text = wide(&name);
    // SAFETY: a NUL-terminated name; the handle is closed below.
    let opens = match unsafe { OpenWindowStationW(PCWSTR(text.as_ptr()), false, READ_CONTROL) } {
        Ok(opened) => {
            // SAFETY: the station was just opened and is closed once, here.
            let _leak_on_failure = unsafe { CloseWindowStation(opened) }.is_ok();
            "1".to_owned()
        }
        Err(e) => format!("err{}", code(&e)),
    };
    format!("name={name} opens={opens}")
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

/// The helper's own value of a system variable, or `fallback`.
fn system_var(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_unset| fallback.to_owned())
}
