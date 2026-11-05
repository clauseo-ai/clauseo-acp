//! [`SpawnOptions::end_with_parent`] on Linux: a child started with it is killed when the thread
//! that started it ends without killing it (the handle leaked, as a crash leaves it), and one
//! started without it lives on. Linux only: Windows has its Job Object, macOS has no such signal.
#![cfg(target_os = "linux")]
#![expect(
    clippy::disallowed_methods,
    reason = "a real child: the test polls for its end with the wall clock"
)]

#[cfg(test)]
mod parent_death {
    use std::collections::BTreeMap;
    use std::mem::ManuallyDrop;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use clauseo_process::{LaunchSpec, SpawnOptions, kill, running, spawn_in_tree};

    /// How long the kernel has to end the child once its parent thread is gone.
    const DEADLINE: Duration = Duration::from_secs(10);

    /// `sleep 600`, from wherever this system keeps it.
    fn sleeper() -> LaunchSpec {
        let program = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.is_file())
            .expect("a sleep binary");
        LaunchSpec::new(
            program.to_owned(),
            vec!["600".into()],
            BTreeMap::new(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        )
        .expect("a valid spec")
    }

    /// Starts the sleeper with `opts` on a thread of its own, leaks its handle (so no drop kills
    /// it), lets the thread end and gives the child's pid.
    fn start_on_a_thread_that_ends(opts: SpawnOptions) -> u32 {
        std::thread::spawn(move || {
            let launched = spawn_in_tree(&sleeper(), &opts).expect("the sleeper starts");
            // Never dropped: the handle's drop would kill it.
            let launched = ManuallyDrop::new(launched);
            launched.handle.pid()
        })
        .join()
        .expect("the starting thread")
    }

    /// Whether `pid` stops running within [`DEADLINE`].
    fn ends_within_deadline(pid: u32) -> bool {
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            if !running(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_child_ends_with_the_thread_that_started_it() {
        let pid = start_on_a_thread_that_ends(SpawnOptions::new().end_with_parent());
        let ended = ends_within_deadline(pid);
        kill(pid);
        assert!(ended, "the child outlived its parent thread");
    }

    #[test]
    fn without_the_option_a_child_outlives_its_thread() {
        let pid = start_on_a_thread_that_ends(SpawnOptions::new());
        std::thread::sleep(Duration::from_millis(300));
        let alive = running(pid);
        kill(pid);
        assert!(alive, "the control child ended by itself");
    }
}
