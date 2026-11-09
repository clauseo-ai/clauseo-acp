//! The browser profile's pipe launches (`BrowserLauncher::launch_chrome`, decision 0038):
//! `DevTools` over Chrome's fds 3 and 4, with no inet listener anywhere in the profile. A
//! stand-in Chrome checks the wiring on every Mac; chrome-headless-shell, when one is found,
//! renders a page and screenshots it over the pipe; agent-browser's daemon, when installed,
//! starts without its stream server.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::PipeReader;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use clauseo_process::{LaunchSpec, Launched};
use clauseo_sandbox::macos::browser::ChromeLaunch;
use clauseo_sandbox::macos::browser::pipe::{MessageReader, write_message};
use futures::AsyncReadExt as _;
use futures::future::Either;
use serde_json::{Value, json};
use std::pin::pin;
use std::time::Duration;

use crate::browser::{Browser, listen, port, s};
use crate::support::Output;

/// How long a pipe launch may run before its tree is killed and the test fails.
const DEADLINE: Duration = Duration::from_secs(60);

/// A stand-in Chrome: reads one NUL-ended message on fd 3 and answers on fd 4 with its own
/// arguments and the message.
const STAND_IN: &str = r#"#!/bin/bash
IFS= read -r -d '' message <&3
printf 'args=%s|got=%s\0' "$*" "$message" >&4
"#;

/// Starts `spec` through `launch_chrome`, runs `talk` on the pipe on a thread of its own while
/// this one drains Chrome's stdout and stderr, then waits for Chrome's exit: `talk` drops its
/// end of the pipe when it returns (or panics), which Chrome takes as the end.
fn with_chrome<T: Send + 'static>(
    b: &Browser,
    spec: &LaunchSpec,
    talk: impl FnOnce(&mut Cdp) -> T + Send + 'static,
) -> (T, Output) {
    let ChromeLaunch { launched, pipe } = b.launcher.launch_chrome(spec).unwrap();
    let (to, from) = pipe.split();
    let talking = std::thread::spawn(move || {
        let mut cdp = Cdp {
            to,
            from,
            next: 0,
            events: Vec::new(),
        };
        talk(&mut cdp)
    });
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launched;
    drop(stdin);
    let output = async_io::block_on(async {
        let mut out = Vec::new();
        let mut err = String::new();
        let report = {
            let read = stdout.read_to_end(&mut out);
            let pumped = stderr.lines(|line| {
                err.push_str(line);
                err.push('\n');
            });
            let all = pin!(async { futures::join!(read, pumped, handle.exited()) });
            let deadline = pin!(async_io::Timer::after(DEADLINE));
            match futures::future::select(all, deadline).await {
                Either::Left(((read, pumped, report), _)) => {
                    read.unwrap();
                    pumped.unwrap();
                    Some(report)
                }
                Either::Right(_) => None,
            }
        };
        Output {
            code: report.and_then(|r| r.code()),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: err,
        }
    });
    // Past the deadline the handle drops here and kills Chrome's tree, which ends the talk.
    drop(handle);
    match talking.join() {
        Ok(talked) => (talked, output),
        Err(panic) => {
            eprintln!("Chrome's output: {output:#?}");
            std::panic::resume_unwind(panic)
        }
    }
}

/// This process's side of a `DevTools` pipe.
struct Cdp {
    to: std::io::PipeWriter,
    from: MessageReader<PipeReader>,
    next: u64,
    /// Events read while waiting for a reply, oldest first.
    events: Vec<Value>,
}

impl Cdp {
    /// The next raw message.
    fn message(&mut self) -> Vec<u8> {
        self.from
            .next_message()
            .unwrap()
            .expect("Chrome closed its pipe")
    }

    /// Sends `method` with `params` (to `session` when given) and returns its result, keeping
    /// the events that come first for [`Cdp::event`].
    fn call(&mut self, method: &str, params: &Value, session: Option<&str>) -> Value {
        self.next += 1;
        let mut command = json!({"id": self.next, "method": method, "params": params});
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        write_message(&mut self.to, command.to_string().as_bytes()).unwrap();
        loop {
            let reply: Value = serde_json::from_slice(&self.message()).unwrap();
            if reply["id"] == json!(self.next) {
                assert!(reply.get("error").is_none(), "{method}: {reply}");
                return reply["result"].clone();
            }
            self.events.push(reply);
        }
    }

    /// Waits for the event `method` of `session`, kept or still to come.
    fn event(&mut self, method: &str, session: &str) -> Value {
        let wanted = |e: &Value| e["method"] == json!(method) && e["sessionId"] == json!(session);
        if let Some(at) = self.events.iter().position(wanted) {
            return self.events.remove(at);
        }
        loop {
            let event: Value = serde_json::from_slice(&self.message()).unwrap();
            if wanted(&event) {
                return event;
            }
        }
    }
}

fn spec(program: &Path, args: &[&str], cwd: &Path) -> LaunchSpec {
    let args = args.iter().map(OsString::from).collect();
    LaunchSpec::new(program.to_owned(), args, BTreeMap::new(), cwd.to_owned()).unwrap()
}

/// The launcher hands Chrome its pipe at fds 3 and 4 through `sandbox-exec`, under the
/// profile, with `--remote-debugging-pipe` first and the port flag gone.
#[test]
fn chrome_gets_its_pipe_at_fds_3_and_4_and_no_port() {
    let b = Browser::new(None);
    let chrome = b.w.executable("chrome/chs/chrome-headless-shell", STAND_IN);
    let flags = ["--headless", "--remote-debugging-port=9222", "about:blank"];
    let (reply, out) = with_chrome(&b, &spec(&chrome, &flags, &b.run), |cdp| {
        write_message(&mut cdp.to, br#"{"id":1}"#).unwrap();
        String::from_utf8(cdp.message()).unwrap()
    });
    out.assert_ok();
    assert_eq!(
        reply,
        r#"args=--remote-debugging-pipe --headless about:blank|got={"id":1}"#
    );
}

/// chrome-headless-shell, the one browser the profile supports on macOS, renders a page in the
/// run folder and screenshots it, driven over its pipe alone, with `--no-sandbox`, the one
/// place that flag is allowed. Skipped when none is found (`CLAUSEO_TEST_HEADLESS_SHELL`,
/// agent-browser's or Playwright's).
#[test]
fn the_headless_shell_screenshots_a_local_page_over_its_pipe() {
    let Some(shell) = crate::bridge::headless_shell() else {
        eprintln!("skipped: no chrome-headless-shell (set CLAUSEO_TEST_HEADLESS_SHELL)");
        return;
    };
    let b = Browser::new(Some(shell.parent().unwrap()));
    let page = b.w.file(
        "data/browser-runs/r1/staging/page.html",
        "<html><body><h1>browser profile</h1></body></html>\n",
    );
    let profile = format!("--user-data-dir={}", s(&b.run.join("profile")));
    let flags = [
        "--headless",
        "--no-sandbox",
        "--disable-gpu",
        &profile,
        "about:blank",
    ];
    let url = format!("file://{}", s(&page));
    let ((heading, png), out) = with_chrome(&b, &spec(&shell, &flags, &b.run), move |cdp| {
        let version = cdp.call("Browser.getVersion", &json!({}), None);
        assert!(version["product"].is_string(), "{version}");
        let blank = json!({"url": "about:blank"});
        let target = cdp.call("Target.createTarget", &blank, None);
        let attach = json!({"targetId": target["targetId"], "flatten": true});
        let session = cdp.call("Target.attachToTarget", &attach, None);
        let session = session["sessionId"].as_str().unwrap().to_owned();
        cdp.call("Page.enable", &json!({}), Some(&session));
        let went = cdp.call("Page.navigate", &json!({"url": url}), Some(&session));
        assert!(went.get("errorText").is_none(), "{went}");
        cdp.event("Page.loadEventFired", &session);
        let read = "document.querySelector('h1').textContent";
        let eval = json!({"expression": read, "returnByValue": true});
        let heading = cdp.call("Runtime.evaluate", &eval, Some(&session));
        let shot = cdp.call("Page.captureScreenshot", &json!({}), Some(&session));
        let png = base64::engine::general_purpose::STANDARD
            .decode(shot["data"].as_str().unwrap())
            .unwrap();
        (heading["result"]["value"].clone(), png)
    });
    assert_eq!(heading, json!("browser profile"), "{out:#?}");
    assert!(png.starts_with(b"\x89PNG"), "no screenshot: {out:#?}");
}

/// agent-browser's daemon under the profile: its stream server's binds (`127.0.0.1` on the
/// given port, then on port 0) are refused, it logs "Stream server failed to start" and stays
/// up, and its Unix socket in `ab/` is made. Skipped when agent-browser is not installed
/// (`command -v agent-browser`).
#[test]
fn agent_browser_s_daemon_stays_up_without_its_stream_server() {
    let Some(daemon) = agent_browser() else {
        eprintln!("skipped: no agent-browser (set CLAUSEO_TEST_AGENT_BROWSER)");
        return;
    };
    let b = Browser::with(None, std::slice::from_ref(&daemon));
    let ab = b.run.join("ab");
    let stream = port(&listen()).to_string();
    // With AGENT_BROWSER_DEBUG the daemon logs to `default.log` in its socket folder.
    let script = r#""$1" & daemon=$!
        sleep 3
        kill -0 "$daemon" && echo DAEMON-UP
        [ -S "$2/default.sock" ] && echo SOCKET-MADE
        cat "$2/default.log"
        kill -KILL "$daemon"; wait "$daemon"; true"#;
    let env = [
        ("AGENT_BROWSER_DAEMON", "1"),
        ("AGENT_BROWSER_DEBUG", "1"),
        ("AGENT_BROWSER_SOCKET_DIR", s(&ab)),
        ("AGENT_BROWSER_STREAM_PORT", stream.as_str()),
    ];
    let all = ["-c", script, "bash", s(&daemon), s(&ab)];
    let out = crate::support::run(&b.launcher, "/bin/bash", &all, &b.run, &env);
    assert!(out.stdout.contains("DAEMON-UP"), "{out:#?}");
    assert!(out.stdout.contains("SOCKET-MADE"), "{out:#?}");
    assert!(
        out.stdout.contains("Stream server failed to start")
            && out.stdout.contains("Operation not permitted"),
        "{out:#?}"
    );
}

/// agent-browser's native binary: `CLAUSEO_TEST_AGENT_BROWSER`, else the `agent-browser` on
/// `PATH`, or, when that is its Node wrapper, the native binary its package ships beside it
/// (npm) or in Homebrew's `libexec`.
fn agent_browser() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUSEO_TEST_AGENT_BROWSER") {
        return Some(PathBuf::from(path));
    }
    let path = std::env::var("PATH").ok()?;
    let found = path
        .split(':')
        .map(|dir| Path::new(dir).join("agent-browser"))
        .find(|p| p.is_file())?;
    let found = dunce::canonicalize(found).ok()?;
    let head = std::fs::read(&found).ok()?;
    if !head.starts_with(b"#!") {
        return Some(found);
    }
    let dir = found.parent()?;
    let native = "agent-browser-darwin-arm64";
    [
        dir.join(native),
        dir.join("../libexec/lib/node_modules/agent-browser/bin")
            .join(native),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .and_then(|p| dunce::canonicalize(p).ok())
}
