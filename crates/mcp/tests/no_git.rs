//! The built `clauseo-mcp` copied as `git` (`git.exe` on Windows), the way the desktop copies it
//! for the agents' `PATH`: it prints nothing and exits 1, except 0 for its one answered command
//! line (`no_git::ANSWERED_OK`).

/// The tests and their helpers, test code throughout (so their `unwrap`s are a test's).
#[cfg(test)]
mod run_as_git {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};

    /// The binary under test.
    const MAIN: &str = env!("CARGO_BIN_EXE_clauseo-mcp");

    /// The scratch folder's writes: the one place this test changes files, since `clippy.toml` bans
    /// std's writes everywhere else.
    #[expect(
        clippy::disallowed_methods,
        reason = "this test's scratch folder changes files here alone; clippy.toml bans std's writes elsewhere"
    )]
    mod write {
        use std::io;
        use std::path::Path;

        /// `fs::copy`.
        pub(crate) fn copy(from: &Path, to: &Path) -> io::Result<u64> {
            std::fs::copy(from, to)
        }

        /// `fs::create_dir_all`.
        pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
            std::fs::create_dir_all(path)
        }

        /// `fs::remove_dir_all`.
        pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
            std::fs::remove_dir_all(path)
        }
    }

    /// Runs `command` to its end with stdin closed, and returns its output.
    #[expect(
        clippy::disallowed_methods,
        reason = "this test runs the built binary here alone; agents and tools start through the process crate"
    )]
    fn run(command: &mut Command) -> Output {
        command.stdin(Stdio::null()).output().unwrap()
    }

    /// A folder for one test, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("clauseo-mcp-no-git-{test}-{}", std::process::id()));
            if path.exists() {
                write::remove_dir_all(&path).unwrap();
            }
            write::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(write::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn run_as_git_it_prints_nothing_and_fails_all_but_one_command_line() {
        let scratch = Scratch::new("stub");
        let tools = scratch.0.join("no-git");
        write::create_dir_all(&tools).unwrap();
        let stub = tools.join(format!("git{}", std::env::consts::EXE_SUFFIX));
        write::copy(Path::new(MAIN), &stub).unwrap();
        let calls: [&[&str]; 11] = [
            &[],
            &["--version"],
            &[
                "check-ignore",
                "-q",
                "--no-index",
                "--",
                ".claude/.cc-writes/sample-file.txt",
            ],
            &["check-ignore", "--stdin", "-z"],
            &["check-ignore", "--", "brief.docx"],
            &[
                "check-ignore",
                "--",
                ".claude/.cc-writes/sample-file.txt",
                "x",
            ],
            &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
            &["-c", "core.hooksPath=/dev/null", "status", "--porcelain"],
            &["diff", "HEAD"],
            &["st"],
            &["browser-guard", "--cwd", "."],
        ];
        for args in calls {
            let output = run(Command::new(&stub).args(args).current_dir(&scratch.0));
            assert_eq!(output.status.code(), Some(1), "{args:?}");
            assert!(output.stdout.is_empty(), "{args:?}: stdout");
            assert!(output.stderr.is_empty(), "{args:?}: stderr");
        }
        let answered = run(Command::new(&stub)
            .args(clauseo_mcp::no_git::ANSWERED_OK)
            .current_dir(&scratch.0));
        assert_eq!(answered.status.code(), Some(0));
        assert!(answered.stdout.is_empty() && answered.stderr.is_empty());
    }

    #[test]
    fn under_its_own_name_it_still_takes_its_modes() {
        let output = run(&mut Command::new(MAIN));
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("usage: clauseo-mcp"), "{stderr:?}");
    }
}
