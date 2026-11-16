//! The `git` that does nothing. The desktop copies this binary to `<data>/tools/no-git/git`
//! (`git.exe` on Windows; `clauseo_profile::tools::install_no_git`), and the host puts that
//! folder first on every agent's `PATH`, so a `git` the agent starts, or one started for it,
//! finds this program. Run under that name, `main` asks [`requested`] before anything else and
//! exits with [`exit_code`] of its arguments: [`EXIT`], or 0 for [`ANSWERED_OK`] alone. It
//! reads no variable, no file and no stdin, prints nothing and starts nothing. Clauseo's own
//! history never runs a `git` program (`clauseo-matters` uses gix in-process), and the host's
//! own `PATH` never holds the folder.

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The file stem the copy is run by.
pub const NAME: &str = "git";

/// The exit code: a failure, as git gives for a command it cannot run.
pub const EXIT: i32 = 1;

/// The one whole command line answered with success, exit 0 and no output, so Claude Code doesn't
/// write a line into the lawyer's global gitignore in their home: Claude Code (2.1.293) runs it
/// in a matter inside a git repository before it adds that line, and adds nothing when it
/// succeeds. Only the whole command line matches; any other `check-ignore` fails, since its
/// success tells the caller a path is ignored.
pub const ANSWERED_OK: [&str; 3] = ["check-ignore", "--", ".claude/.cc-writes/sample-file.txt"];

/// The exit code for the arguments after the program: 0 for exactly [`ANSWERED_OK`], else
/// [`EXIT`].
pub fn exit_code(args: &[OsString]) -> i32 {
    let answered = args.len() == ANSWERED_OK.len()
        && args
            .iter()
            .zip(ANSWERED_OK)
            .all(|(arg, want)| arg.as_os_str() == want);
    if answered { 0 } else { EXIT }
}

/// Whether the program runs as the `git` that does nothing: the file stem of `exe` (the running
/// executable, `std::env::current_exe`) or of `argv0` (the name it was started by) is [`NAME`],
/// ignoring ASCII case (Windows does). Either one is enough: on Linux a replaced copy's
/// `current_exe` ends in ` (deleted)`, and a caller may start it by any name.
pub fn requested(exe: Option<&Path>, argv0: Option<&OsStr>) -> bool {
    let named = |path: &Path| {
        path.file_stem()
            .is_some_and(|stem| stem.eq_ignore_ascii_case(NAME))
    };
    exe.is_some_and(named) || argv0.map(Path::new).is_some_and(named)
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    use super::{ANSWERED_OK, EXIT, exit_code, requested};

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_the_one_answered_command_line_succeeds() {
        assert_eq!(exit_code(&args(&ANSWERED_OK)), 0);
        for other in [
            &[][..],
            &["--version"],
            &["check-ignore"],
            &["check-ignore", "--", "brief.docx"],
            &["check-ignore", ".claude/.cc-writes/sample-file.txt"],
            &[
                "check-ignore",
                "--",
                ".claude/.cc-writes/sample-file.txt",
                "x",
            ],
            &[
                "-C",
                "x",
                "check-ignore",
                "--",
                ".claude/.cc-writes/sample-file.txt",
            ],
            &["status", "--porcelain"],
        ] {
            assert_eq!(exit_code(&args(other)), EXIT, "{other:?}");
        }
    }

    #[test]
    fn only_a_program_named_git_does_nothing() {
        for exe in [
            "/data/tools/no-git/git",
            "C:\\data\\tools\\no-git\\git.exe",
            "/x/GIT",
            "git",
        ] {
            assert!(requested(Some(Path::new(exe)), None), "{exe}");
            assert!(requested(None, Some(OsStr::new(exe))), "{exe}");
        }
        for exe in [
            "/app/clauseo-mcp",
            "C:\\app\\clauseo-mcp.exe",
            "/x/git-lfs",
            "/x/gitk",
            "/x/git (deleted)",
            "/x/.git",
            "",
        ] {
            assert!(!requested(Some(Path::new(exe)), None), "{exe}");
            assert!(!requested(None, Some(OsStr::new(exe))), "{exe}");
        }
        assert!(requested(
            Some(Path::new("/x/git (deleted)")),
            Some(OsStr::new("/data/tools/no-git/git"))
        ));
        assert!(!requested(None, None));
    }
}
