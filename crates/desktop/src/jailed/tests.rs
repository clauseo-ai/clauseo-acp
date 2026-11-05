use std::cell::Cell;
use std::ffi::OsString;
use std::io;

use super::{Launched, REFUSAL, SANDBOXED_EXIT, SandboxProbe, Stages, SystemProbe, launch};

/// No guard run.
const NO_GUARD: [Vec<OsString>; 0] = [];

/// A probe with a fixed answer, counting the times it was asked.
struct Probe {
    answer: fn() -> io::Result<bool>,
    asked: Cell<u32>,
}

impl Probe {
    fn new(answer: fn() -> io::Result<bool>) -> Self {
        Self {
            answer,
            asked: Cell::new(0),
        }
    }
}

impl SandboxProbe for Probe {
    fn sandboxed(&self) -> io::Result<bool> {
        self.asked.set(self.asked.get() + 1);
        (self.answer)()
    }
}

/// Stages that count their calls and keep the guard's arguments.
#[derive(Default)]
struct Counted {
    guards: Vec<Vec<OsString>>,
    apps: u32,
}

impl Stages for Counted {
    fn guard(&mut self, args: &[OsString]) -> i32 {
        self.guards.push(args.to_vec());
        2
    }

    fn app(&mut self) -> tauri::Result<()> {
        self.apps += 1;
        Ok(())
    }
}

/// Runs `launch` on `args` with `probe`; returns the outcome, stderr and the stages.
fn start(args: &[&str], probe: &Probe) -> (Launched, String, Counted) {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let mut stderr = Vec::new();
    let mut stages = Counted::default();
    let launched = launch(&args, probe, &mut stderr, &mut stages);
    (launched, String::from_utf8(stderr).unwrap(), stages)
}

#[test]
fn sandboxed_refuses_with_its_code_and_line_and_never_reaches_the_app() {
    // A deep link, as a second launch would forward it to the running window.
    let probe = Probe::new(|| Ok(true));
    let (launched, stderr, stages) = start(&["clauseo://open"], &probe);
    assert!(
        matches!(launched, Launched::Exit(SANDBOXED_EXIT)),
        "{launched:?}"
    );
    assert_eq!(SANDBOXED_EXIT, 77);
    assert_eq!(stderr, "Clauseo will not start inside the agent sandbox.\n");
    assert_eq!(stderr, format!("{REFUSAL}\n"));
    assert_eq!(stages.apps, 0);
    assert_eq!(stages.guards, NO_GUARD);
    assert_eq!(probe.asked.get(), 1);
}

#[test]
fn not_sandboxed_starts_the_app_once_and_says_nothing() {
    for args in [&[][..], &["clauseo://open"][..]] {
        let probe = Probe::new(|| Ok(false));
        let (launched, stderr, stages) = start(args, &probe);
        assert!(matches!(launched, Launched::App(Ok(()))), "{launched:?}");
        assert_eq!(stderr, "");
        assert_eq!(stages.apps, 1);
        assert_eq!(stages.guards, NO_GUARD);
        assert_eq!(probe.asked.get(), 1);
    }
}

#[test]
fn a_failed_check_says_so_once_and_starts_the_app() {
    let probe = Probe::new(|| Err(io::Error::other("access denied")));
    let (launched, stderr, stages) = start(&[], &probe);
    assert!(matches!(launched, Launched::App(Ok(()))), "{launched:?}");
    assert_eq!(
        stderr,
        "Clauseo could not tell whether it runs in the agent sandbox (access denied); starting.\n"
    );
    assert_eq!(stages.apps, 1);
    assert_eq!(probe.asked.get(), 1);
}

#[test]
fn the_guard_runs_inside_the_sandbox_without_asking() {
    let probe = Probe::new(|| Ok(true));
    let (launched, stderr, stages) = start(&["--pre-write-guard", "--matter", "/m"], &probe);
    assert!(matches!(launched, Launched::Exit(2)), "{launched:?}");
    assert_eq!(stderr, "");
    assert_eq!(
        stages.guards,
        [vec![OsString::from("--matter"), OsString::from("/m")]]
    );
    assert_eq!(stages.apps, 0);
    assert_eq!(probe.asked.get(), 0);
}

#[test]
fn the_guard_flag_anywhere_but_first_is_refused_when_sandboxed() {
    let probe = Probe::new(|| Ok(true));
    let (launched, _, stages) = start(&["clauseo://open", "--pre-write-guard"], &probe);
    assert!(
        matches!(launched, Launched::Exit(SANDBOXED_EXIT)),
        "{launched:?}"
    );
    assert_eq!(stages.guards, NO_GUARD);
    assert_eq!(stages.apps, 0);
}

/// The real calls run on each CI OS (on Windows and macOS this is the only run of the FFI). Only
/// "no error" is asserted: a developer may run the tests inside some other sandbox.
#[test]
fn the_system_probe_answers_without_an_error() {
    let answer = SystemProbe.sandboxed();
    assert!(answer.is_ok(), "{answer:?}");
    #[cfg(not(any(windows, target_os = "macos")))]
    assert!(!answer.unwrap());
}
