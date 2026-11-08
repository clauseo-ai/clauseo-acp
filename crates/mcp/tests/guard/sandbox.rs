//! Chrome's sandbox, never weakened (decision record 0029, "Chrome's sandbox, never weakened"):
//! a Chrome whose sandbox cannot start is refused with the plain sentence and never started
//! again, and the guard refuses agent-browser arguments that name a switch weakening it.

use std::ffi::OsStr;

use crate::support::{Guard, Scratch, example, guard_command, run, run_files};

/// What the agent's call gets.
const REFUSAL: &str =
    "The browser couldn't start its own protection on this computer, so Clauseo won't use it.";

/// One line of each sandbox failure the guard knows (`src/browser_guard/chrome/sandbox.rs`), as
/// Chrome prints it, with a short name for the scratch folder.
const FAILURES: &[(&str, &str)] = &[
    (
        "usable",
        "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(146)] No usable sandbox! If you \
         are running on Ubuntu 23.10+ ...",
    ),
    (
        "root",
        "[1:1:0101/000000.000000:ERROR:zygote_host_impl_linux.cc(118)] Running as root without \
         --no-sandbox is not supported. See https://crbug.com/638180.",
    ),
    (
        "zyexit",
        "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(244)] Zygote process exited \
         prematurely with exit code 1",
    ),
    (
        "zyboot",
        "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(248)] Failed to receive boot \
         message from zygote",
    ),
    (
        "zyhello",
        "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(273)] Failed to receive hello \
         message from zygote",
    ),
    (
        "ns",
        "Failed to move to new namespace: PID namespaces supported, Network namespace supported, \
         but failed: errno = Operation not permitted",
    ),
    (
        "suidroot",
        "The setuid sandbox is not running as root. Common causes:",
    ),
    (
        "suidmissing",
        "[1:1:0101/000000.000000:FATAL:setuid_sandbox_host.cc(163)] The SUID sandbox helper binary \
         is missing: /opt/google/chrome/chrome-sandbox Aborting now.",
    ),
    (
        "suidclient",
        "[1:1:0101/000000.000000:FATAL:sandbox_linux.cc(154)] Failed to instantiate the setuid \
         sandbox client.",
    ),
    (
        "nozygote",
        "[1:1:0101/000000.000000:ERROR:content_main_runner_impl.cc(360)] Zygote cannot be disabled \
         if sandbox is enabled. Use --no-zygote together with --no-sandbox",
    ),
    (
        "win",
        "[1:1:0101/000000.000:ERROR:sandbox_win.cc(805)] Sandbox cannot access executable \
         C:\\Chrome\\chrome.exe. Check filesystem permissions are valid.",
    ),
    (
        "mac",
        "SeatbeltExecClient: pipe failed: Too many open files",
    ),
];

/// How many times the Chrome stand-in started.
fn launches(scratch: &Scratch) -> usize {
    std::fs::read_to_string(scratch.path().join("chrome-launches"))
        .unwrap_or_default()
        .lines()
        .count()
}

/// Two tool calls at once with Chrome in `mode`: the first is refused with the plain sentence,
/// the guard exits 1 saying why, and Chrome started once, never again.
fn refused(name: &str, mode: &str) -> String {
    let scratch = Scratch::new(&format!("sandbox_{name}"));
    scratch.chrome_mode(mode);
    let log = scratch.log();
    let mut guard = Guard::start(guard_command(
        &scratch,
        &example("echo_stub"),
        &[log.as_os_str()],
    ));
    guard.send_call("open");
    guard.send_call("again");
    let answer = guard.read();
    assert_eq!(answer["id"], "open", "{answer}");
    assert_eq!(answer["error"]["code"], -32603, "{answer}");
    assert_eq!(answer["error"]["message"], REFUSAL, "{answer}");
    let (status, (rest, stderr)) = guard.finish_with_stderr();
    assert_eq!(status.code(), Some(1), "{stderr}");
    assert!(
        !rest.contains(REFUSAL),
        "one refusal, no second start: {rest}"
    );
    assert_eq!(launches(&scratch), 1, "Chrome started again: {stderr}");
    assert!(!log.exists(), "agent-browser never started");
    assert_eq!(run_files(&scratch.cwd()), Vec::<std::path::PathBuf>::new());
    stderr
}

#[test]
fn chrome_that_exits_with_a_sandbox_failure_is_refused_and_never_started_again() {
    for (name, line) in FAILURES {
        let stderr = refused(name, &format!("sandbox-exit:{line}"));
        let last = stderr.trim_end().lines().last().unwrap_or_default();
        assert!(
            last.starts_with("clauseo-mcp browser-guard: Chrome's sandbox could not start")
                && last.ends_with("the guard never runs Chrome without it"),
            "{name}: {stderr}"
        );
        // The fixed part only: never the path some lines carry.
        assert!(
            !last.contains("chrome-sandbox") && !last.contains("C:\\"),
            "{last}"
        );
    }
}

/// Windows' broker failure leaves Chrome running; the guard refuses it all the same and kills
/// it (the guard's exit ends Chrome's tree, `lifecycle.rs`).
#[test]
fn chrome_that_reports_a_sandbox_failure_and_goes_on_is_refused() {
    let (_, line) = FAILURES[10];
    refused("stay", &format!("sandbox-stay:{line}"));
}

/// The guard's own command line: agent-browser's arguments (what the host passes after `--`)
/// naming a weakening switch are refused before anything starts.
#[test]
fn agent_browser_arguments_naming_a_weakening_switch_are_refused() {
    for (name, args) in [
        ("args", &["mcp", "--args", "--no-sandbox"][..]),
        (
            "argseq",
            &["mcp", "--args=--headless=new,--Disable-GPU-Sandbox"][..],
        ),
        ("single", &["mcp", "--single-process"][..]),
        (
            "features",
            &["mcp", "--args", "--disable-features=NetworkServiceSandbox"][..],
        ),
    ] {
        let scratch = Scratch::new(&format!("sandbox_child_{name}"));
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let output = run(guard_command(&scratch, &example("echo_stub"), &args));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{name}: {stderr}");
        assert!(
            stderr.contains("which weakens Chrome's sandbox"),
            "{name}: {stderr}"
        );
        assert_eq!(launches(&scratch), 0, "{name}");
    }
}

/// Chrome that exits at its start with nothing the guard recognises is not started again either:
/// the call gets the ordinary failed start, not the sandbox's sentence, and the guard exits 1
/// with the existing failed-start error.
#[test]
fn chrome_that_exits_early_with_unknown_text_is_not_started_again() {
    let scratch = Scratch::new("sandbox_unknown");
    scratch.chrome_mode(
        "sandbox-exit:[1:1:0101/000000.000000:FATAL:somewhere.cc(1)] Something else went wrong",
    );
    let log = scratch.log();
    let mut guard = Guard::start(guard_command(
        &scratch,
        &example("echo_stub"),
        &[log.as_os_str()],
    ));
    guard.send_call("open");
    guard.send_call("again");
    let answer = guard.read();
    assert_eq!(answer["id"], "open", "{answer}");
    assert_eq!(answer["error"]["code"], -32603, "{answer}");
    assert_ne!(answer["error"]["message"], REFUSAL, "{answer}");
    let (status, (_, stderr)) = guard.finish_with_stderr();
    assert_eq!(status.code(), Some(1), "{stderr}");
    let last = stderr.trim_end().lines().last().unwrap_or_default();
    assert!(
        last.starts_with(
            "clauseo-mcp browser-guard: Chrome stopped before it listened on the claimed \
             debugging port"
        ) && last.contains("Something else went wrong"),
        "{stderr}"
    );
    assert_eq!(launches(&scratch), 1, "Chrome started again: {stderr}");
    assert!(!log.exists(), "agent-browser never started");
}
