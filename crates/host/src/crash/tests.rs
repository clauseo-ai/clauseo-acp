//! What a settings view says about sending crash reports.

use super::switch_of;

fn locked(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

#[test]
fn crash_reports_follow_counts_unless_policy_locks_them_off() {
    let open = switch_of(true, &locked(&["ui.theme"]));
    assert!(open.may_send());
    assert!(!switch_of(false, &[]).may_send());
    for keys in [
        &["privacy.crash_reports"][..],
        &["privacy."],
        &["ui.theme", "privacy."],
    ] {
        let switch = switch_of(true, &locked(keys));
        assert!(switch.locked_off, "{keys:?}");
        assert!(switch.counts);
        assert!(!switch.may_send());
    }
    for keys in [
        &["privacy.counts"][..],
        &["privacy.crash"],
        &["plans."],
        &["privacy"],
    ] {
        assert!(!switch_of(true, &locked(keys)).locked_off, "{keys:?}");
    }
}

/// The native handler, in a child process of this test binary that crashes (or is killed) for
/// real. Unix: a fault the kernel raises and `abort` leave a report at the next start; a kill
/// leaves none, and on Linux neither does a fault signal sent to it (macOS records that one).
/// Windows: an access violation and a stack overflow reach the unhandled-exception filter and
/// leave one report, the process still ending with Windows' code; `abort` fails fast past the
/// filter and `TerminateProcess` raises nothing, so neither leaves one.
#[cfg(any(unix, windows))]
mod native {
    use std::path::Path;
    use std::process::{Child, ExitStatus};
    use std::sync::mpsc;
    use std::time::Duration;

    use clauseo_counts::schema::{
        Arch, Channel, CrashKind, InstallType, Language, Platform, Time, parse_crash,
    };
    use clauseo_counts::{About, Counts, Crashes, Switch};

    const MODE: &str = "CLAUSEO_CRASH_CHILD";
    const DIR: &str = "CLAUSEO_CRASH_CHILD_DIR";
    const ON: Switch = Switch {
        counts: true,
        locked_off: false,
    };
    /// How long a child may take to end, in looks `LOOK` apart; longer fails the test as a hang.
    const ENDS_WITHIN: u32 = 750;
    /// The time between two looks at a child: 750 of them are 15 s.
    const LOOK: Duration = Duration::from_millis(20);
    /// The file a child that waits to be killed makes once its handler is in.
    const READY: &str = "ready";

    fn about() -> About {
        About {
            version: "3.0.4".parse().unwrap(),
            channel: Channel::Stable,
            platform: if cfg!(windows) {
                Platform::Windows(clauseo_counts::schema::OsMajor::new(11).unwrap())
            } else {
                Platform::Linux
            },
            arch: Arch::X86_64,
            language: Language::En,
            install_type: InstallType::PerUser,
            managed: false,
        }
    }

    fn now() -> Time {
        super::super::now()
    }

    /// Counts saving in `dir`, and its crashes ingested once, as desktop has them at start.
    fn start(dir: &Path) -> (Counts, Crashes) {
        let counts = Counts::open(dir, about(), true, true, 1);
        let crashes = Crashes::open(dir, about(), 1);
        crashes.ingest(&counts, ON, now()).unwrap();
        (counts, crashes)
    }

    /// Recurses until the stack runs out.
    fn overflow(depth: u64) -> u64 {
        let frame = std::hint::black_box([1_u8; 4096]);
        if std::hint::black_box(depth) == u64::MAX {
            return 0;
        }
        overflow(depth + 1) + u64::from(frame[0])
    }

    /// The child: does nothing unless the parent set `MODE`.
    #[test]
    #[expect(
        unsafe_code,
        reason = "a real fault: a write through a null pointer; on Windows, no error dialog"
    )]
    fn native_child() {
        let (Ok(mode), Ok(dir)) = (std::env::var(MODE), std::env::var(DIR)) else {
            return;
        };
        #[cfg(windows)]
        {
            use ::windows::Win32::System::Diagnostics::Debug::{
                SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode, THREAD_ERROR_MODE,
            };
            // SAFETY: sets this process's error mode only. Without it Windows Error Reporting
            // may wait on a dialog; the filter runs either way.
            let _previous = unsafe {
                SetErrorMode(THREAD_ERROR_MODE(
                    SEM_FAILCRITICALERRORS.0 | SEM_NOGPFAULTERRORBOX.0,
                ))
            };
        }
        let dir = Path::new(&dir);
        let (_counts, crashes) = start(dir);
        assert!(super::super::record_crashes(&crashes));
        crashes.set_launched();
        let null = std::hint::black_box(std::ptr::null_mut::<u8>());
        match mode.as_str() {
            "fault" => {
                // SAFETY: none; the fault is the point, and the handler records it.
                unsafe { null.write_volatile(1) };
            }
            "fault_after_a_crash_began" => {
                // As a second fault inside the handler finds it: this run's one crash is taken.
                assert!(crashes.begin_crash());
                // SAFETY: as above.
                unsafe { null.write_volatile(1) };
            }
            "overflow" => {
                std::hint::black_box(overflow(0));
            }
            "abort" => std::process::abort(),
            "wait" => {
                crate::write::write_file(&dir.join(READY), b"").unwrap();
                // Waits a minute for the parent's kill; nothing is ever sent.
                let (_keep, never) = mpsc::channel::<()>();
                let _waited = never.recv_timeout(LOOK * ENDS_WITHIN * 4);
            }
            #[cfg(unix)]
            "sent_segv" => {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGSEGV).unwrap();
            }
            #[cfg(unix)]
            "term" => {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGTERM).unwrap();
            }
            #[cfg(unix)]
            "kill" => {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGKILL).unwrap();
            }
            _ => {}
        }
        // Still running: the parent sees a clean exit and fails.
    }

    /// Starts the child in `mode` with its data folder `dir`.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test binary runs itself as the crashing child"
    )]
    fn run_child(mode: &str, dir: &Path) -> Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crash::tests::native::native_child",
                "--nocapture",
            ])
            .env(MODE, mode)
            .env(DIR, dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    /// Waits `LOOK` (on a channel nothing is sent to, as the crate's other tests wait).
    fn look(never: &mpsc::Receiver<()>) {
        let _waited = never.recv_timeout(LOOK);
    }

    /// How `child` ended; a child still running after `ENDS_WITHIN` looks is killed and fails
    /// the test as a hang.
    fn ended(mut child: Child, mode: &str) -> ExitStatus {
        let (_keep, never) = mpsc::channel::<()>();
        for _ in 0..ENDS_WITHIN {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            look(&never);
        }
        drop(child.kill());
        drop(child.wait());
        panic!("{mode}: the child hung");
    }

    /// The codes of the reports the next start finds in `dir`. Each is read through the
    /// schema, which refuses a field it does not know.
    fn next_start_codes(dir: &Path) -> Vec<u32> {
        let (counts, crashes) = start(dir);
        drop(crashes.open_native(now()).unwrap());
        crashes.ingest(&counts, ON, now()).unwrap();
        let week = now().day().unwrap().week();
        crashes
            .queued()
            .iter()
            .map(|name| {
                let bytes = std::fs::read(dir.join("crash/queue").join(name)).unwrap();
                let report = parse_crash(&bytes, week).unwrap();
                assert_eq!(report.kind, CrashKind::Signal);
                assert!(report.after_launch);
                report.code.unwrap().get()
            })
            .collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("clauseo-crash-{name}-{}", std::process::id()));
        crate::write::remove_dir_all(&dir).unwrap();
        crate::write::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn a_kernel_fault_and_an_abort_are_recorded_and_kills_are_not() {
        // A sent SIGSEGV goes on to Rust's own handler, which resets to the default and returns;
        // with no fault to repeat, the child may carry on and exit cleanly, so how it ends is
        // not checked. What is checked: macOS records exactly one report, [11] (XNU's sendsig
        // gives a sent SIGSEGV the same si_code as a fault: bsd/dev/arm/unix_signal.c,
        // bsd/dev/i386/unix_signal.c), and Linux none. The others end by their signal.
        let sent_segv = if cfg!(target_vendor = "apple") {
            vec![nix::libc::SIGSEGV.cast_unsigned()]
        } else {
            vec![]
        };
        let cases = [
            (
                "fault",
                Some(false),
                vec![nix::libc::SIGSEGV.cast_unsigned()],
            ),
            (
                "abort",
                Some(false),
                vec![nix::libc::SIGABRT.cast_unsigned()],
            ),
            ("sent_segv", None, sent_segv),
            // TERM and KILL leave nothing on every OS, macOS included.
            ("term", Some(false), vec![]),
            ("kill", Some(false), vec![]),
        ];
        for (mode, clean, want) in cases {
            let dir = scratch(mode);
            let exited_clean = ended(run_child(mode, &dir), mode).success();
            if let Some(clean) = clean {
                assert_eq!(exited_clean, clean, "{mode}: how the child ended");
            }
            assert_eq!(next_start_codes(&dir), want, "{mode}");
            crate::write::remove_dir_all(&dir).unwrap();
        }
    }

    /// `STATUS_ACCESS_VIOLATION`.
    #[cfg(windows)]
    const ACCESS_VIOLATION: u32 = 0xC000_0005;
    /// `STATUS_STACK_OVERFLOW`.
    #[cfg(windows)]
    const STACK_OVERFLOW: u32 = 0xC000_00FD;
    /// `STATUS_STACK_BUFFER_OVERRUN`, the code of a fail-fast, which `abort` is on Windows.
    #[cfg(windows)]
    const FAIL_FAST: u32 = 0xC000_0409;

    #[cfg(windows)]
    #[test]
    fn an_unhandled_exception_is_recorded_once_and_the_process_still_ends_with_its_code() {
        let cases = [
            ("fault", ACCESS_VIOLATION, vec![ACCESS_VIOLATION]),
            ("overflow", STACK_OVERFLOW, vec![STACK_OVERFLOW]),
            // A second fault inside the filter finds this run's one crash taken
            // (`Crashes::begin_crash`), records nothing and ends the process the same way.
            ("fault_after_a_crash_began", ACCESS_VIOLATION, vec![]),
            // `abort` fails fast: no filter runs (a panic's report comes from the panic hook).
            ("abort", FAIL_FAST, vec![]),
        ];
        for (mode, code, want) in cases {
            let dir = scratch(mode);
            let status = ended(run_child(mode, &dir), mode);
            assert_eq!(
                status.code().map(i32::cast_unsigned),
                Some(code),
                "{mode}: {status}"
            );
            // The next start reads the first line after the header only: the record itself
            // holds exactly as many lines as reports, so a second write would show here.
            let record = std::fs::read_to_string(dir.join("crash/native")).unwrap();
            assert_eq!(
                record.lines().skip(1).count(),
                want.len(),
                "{mode}: {record}"
            );
            assert_eq!(next_start_codes(&dir), want, "{mode}");
            crate::write::remove_dir_all(&dir).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn terminate_process_from_outside_records_nothing() {
        let dir = scratch("terminate");
        let mut child = run_child("wait", &dir);
        let (_keep, never) = mpsc::channel::<()>();
        let mut looks = 0;
        while !dir.join(READY).exists() {
            looks += 1;
            assert!(looks < ENDS_WITHIN, "the child never got ready");
            assert!(child.try_wait().unwrap().is_none(), "the child ended early");
            look(&never);
        }
        // `TerminateProcess`, with exit code 1.
        child.kill().unwrap();
        let status = ended(child, "terminate");
        assert_eq!(status.code(), Some(1), "{status}");
        assert_eq!(next_start_codes(&dir), Vec::<u32>::new());
        crate::write::remove_dir_all(&dir).unwrap();
    }
}
