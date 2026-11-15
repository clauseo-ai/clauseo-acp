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
/// real: a fault the kernel raises and `abort` leave a report at the next start; a kill, or a
/// fault signal another process sent, leave none.
#[cfg(unix)]
mod native {
    use std::path::Path;

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

    fn about() -> About {
        About {
            version: "3.0.4".parse().unwrap(),
            channel: Channel::Stable,
            platform: Platform::Linux,
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

    /// The child: does nothing unless the parent set `MODE`.
    #[test]
    #[expect(unsafe_code, reason = "a real fault: a write through a null pointer")]
    fn native_child() {
        let (Ok(mode), Ok(dir)) = (std::env::var(MODE), std::env::var(DIR)) else {
            return;
        };
        let (_counts, crashes) = start(Path::new(&dir));
        assert!(super::super::record_crashes(&crashes));
        crashes.set_launched();
        match mode.as_str() {
            "segv" => {
                let null = std::hint::black_box(std::ptr::null_mut::<u8>());
                // SAFETY: none; the fault is the point, and the handler records it.
                unsafe { null.write_volatile(1) };
            }
            "abort" => std::process::abort(),
            "sent_segv" => {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGSEGV).unwrap();
            }
            "term" => {
                nix::sys::signal::raise(nix::sys::signal::Signal::SIGTERM).unwrap();
            }
            _ => {}
        }
        // Still running: the parent sees a clean exit and fails.
    }

    /// Runs the child in `mode` with its data folder `dir`; whether it exited cleanly.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test binary runs itself as the crashing child"
    )]
    fn run_child(mode: &str, dir: &Path) -> bool {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crash::tests::native::native_child",
                "--nocapture",
            ])
            .env(MODE, mode)
            .env(DIR, dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        status.success()
    }

    /// The codes of the reports the next start finds in `dir`.
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

    #[test]
    fn a_kernel_fault_and_an_abort_are_recorded_and_kills_are_not() {
        // A sent SIGSEGV goes on to Rust's own handler, which lets a process it was sent to
        // carry on, so that child may exit cleanly; the others end by their signal.
        let cases = [
            (
                "segv",
                Some(false),
                vec![nix::libc::SIGSEGV.cast_unsigned()],
            ),
            (
                "abort",
                Some(false),
                vec![nix::libc::SIGABRT.cast_unsigned()],
            ),
            ("sent_segv", None, vec![]),
            ("term", Some(false), vec![]),
        ];
        for (mode, clean, want) in cases {
            let dir = scratch(mode);
            let exited_clean = run_child(mode, &dir);
            if let Some(clean) = clean {
                assert_eq!(exited_clean, clean, "{mode}: how the child ended");
            }
            assert_eq!(next_start_codes(&dir), want, "{mode}");
            crate::write::remove_dir_all(&dir).unwrap();
        }
    }
}
