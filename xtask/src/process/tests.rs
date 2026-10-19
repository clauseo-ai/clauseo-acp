//! Tests for `process.rs`: how git, pnpm and timed commands run.

use super::*;
#[cfg(unix)]
use crate::scratch::Scratch;
use std::ffi::OsString;

#[test]
fn git_runs_without_the_variables_that_point_it_at_another_repo() {
    let git = Program::Git.command();
    let removed: Vec<&OsStr> = git
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name)
        .collect();
    for name in GIT_LOCAL_ENV {
        assert!(removed.contains(&OsStr::new(name)), "{name} is removed");
    }
    assert_eq!(git.get_program(), "git");
}

#[test]
fn cmd_displays_its_program_then_its_args() {
    let cmd = Cmd::new(Program::Cargo, ["fmt", "--all", "--", "--check"]);
    assert_eq!(cmd.to_string(), "cargo fmt --all -- --check");
    let named = Cmd::new(Program::Named("git"), Vec::<String>::new());
    assert_eq!(named.to_string(), "git");
    let pnpm = Cmd::new(Program::Pnpm, ["--version"]);
    assert_eq!(pnpm.to_string(), "pnpm --version");
}

/// The path of `name` in `dir`.
fn at(dir: &str, name: &str) -> PathBuf {
    Path::new(dir).join(name)
}

/// A PATH value of `dirs`, joined the way the OS joins one.
fn path_of(dirs: &[&str]) -> OsString {
    std::env::join_paths(dirs).expect("test directories hold no separator")
}

/// An `exists` predicate that holds for exactly `files`.
fn only(files: &[PathBuf]) -> impl Fn(&Path) -> bool + '_ {
    move |file| files.iter().any(|held| held == file)
}

/// The directories of the PATH the tests search: the first holds no pnpm, the second one.
const DIRS: [&str; 2] = ["/tools/bin", "/usr/local/node"];

#[test]
fn pnpm_resolves_to_a_cmd_file_on_path() {
    let files = [at("/usr/local/node", "pnpm.cmd")];
    assert_eq!(
        resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
        at("/usr/local/node", "pnpm.cmd")
    );
}

#[test]
fn pnpm_resolves_to_an_exe_when_there_is_no_cmd() {
    let files = [at("/tools/bin", "pnpm.exe")];
    assert_eq!(
        resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
        at("/tools/bin", "pnpm.exe")
    );
}

#[test]
fn pnpm_resolves_to_bare_pnpm_when_neither_is_on_path() {
    assert_eq!(
        resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&[])),
        PathBuf::from("pnpm")
    );
}

#[test]
fn an_exe_in_an_earlier_directory_beats_a_cmd_in_a_later_one() {
    let files = [
        at("/tools/bin", "pnpm.exe"),
        at("/usr/local/node", "pnpm.cmd"),
    ];
    assert_eq!(
        resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
        at("/tools/bin", "pnpm.exe")
    );
}

#[test]
fn an_exe_beats_a_cmd_in_the_same_directory() {
    let files = [
        at("/usr/local/node", "pnpm.cmd"),
        at("/usr/local/node", "pnpm.exe"),
    ];
    assert_eq!(
        resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files)),
        at("/usr/local/node", "pnpm.exe")
    );
}

#[test]
fn the_resolved_path_is_the_directory_joined_with_the_name() {
    let files = [at("/usr/local/node", "pnpm.exe")];
    let resolved = resolve_pnpm(&path_of(&DIRS), &WINDOWS_PNPM, only(&files));
    assert_eq!(resolved.parent(), Some(Path::new("/usr/local/node")));
    assert_eq!(resolved.file_name(), Some(OsStr::new("pnpm.exe")));
}

#[cfg(unix)]
#[test]
fn capture_merges_stdout_and_stderr_and_reports_the_exit_status() {
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "echo out; echo err >&2; exit 3"],
    );
    let mut seen = Vec::new();
    let captured = capture(&cmd, Path::new("."), &mut |chunk| {
        seen.extend_from_slice(chunk);
    })
    .expect("sh runs");
    assert!(!captured.success);
    assert!(captured.output.contains("out"));
    assert!(captured.output.contains("err"));
    assert_eq!(seen, captured.output.as_bytes());
}

#[test]
fn git_local_env_is_what_git_rev_parse_local_env_vars_lists() {
    let cmd = Cmd::new(Program::Git, ["rev-parse", "--local-env-vars"]);
    let listed = run_quiet(&cmd, Path::new(".")).expect("git runs");
    assert!(listed.success, "`{cmd}` failed: {}", listed.stderr);
    let listed: Vec<&str> = listed.stdout.lines().collect();
    assert_eq!(
        listed, GIT_LOCAL_ENV,
        "`{cmd}` lists other variables than GIT_LOCAL_ENV in xtask/src/process.rs: update \
         GIT_LOCAL_ENV to the list git prints"
    );
}

#[test]
fn a_timed_kill_names_the_group_on_unix_for_quiet_and_the_process_for_inherit() {
    let quiet = kill_cmd(4321, Output::Quiet).to_string();
    let inherit = kill_cmd(4321, Output::Inherit).to_string();
    if cfg!(windows) {
        assert_eq!(quiet, "taskkill /PID 4321 /T /F");
        assert_eq!(inherit, "taskkill /PID 4321 /T /F");
    } else {
        assert_eq!(quiet, "kill -KILL -- -4321");
        assert_eq!(inherit, "kill -KILL 4321");
    }
}

#[test]
fn a_timed_command_that_cannot_start_is_an_error() {
    let cmd = Cmd::new(Program::Named("xtask-no-such-program"), ["--version"]);
    let ran = run_timed(&cmd, Path::new("."), Output::Quiet, Duration::from_secs(30));
    assert_eq!(
        ran.expect_err("the program does not exist").kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn a_timed_command_that_exits_gives_its_status_and_output() {
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "echo out; echo err >&2; exit 3"],
    );
    let ran = run_timed(&cmd, Path::new("."), Output::Quiet, Duration::from_secs(30));
    let Ok(Ended::Exited(quiet)) = ran else {
        panic!("sh exits: {ran:?}");
    };
    assert!(!quiet.success);
    assert_eq!(quiet.stdout, "out\n");
    assert_eq!(quiet.stderr, "err\n");
}

/// Whether process `pid` has ended: `ps` finds no such process, or only its zombie.
#[cfg(unix)]
fn has_ended(pid: &str) -> bool {
    let ps = Cmd::new(Program::Named("ps"), ["-o", "stat=", "-p", pid]);
    let found = run_quiet(&ps, Path::new(".")).expect("ps runs");
    !found.success || found.stdout.trim().starts_with('Z')
}

#[cfg(unix)]
#[test]
fn a_quiet_command_past_its_timeout_is_killed_with_the_processes_it_started() {
    let dir = Scratch::new("process-timeout-tree");
    // The grandchild holds the output pipe, so run_timed returns only once it is gone too.
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "sleep 30 & echo $! > grandchild; wait"],
    );
    let (ran, seconds) =
        crate::clock::timed(|| run_timed(&cmd, &dir, Output::Quiet, Duration::from_millis(500)));
    assert!(matches!(ran, Ok(Ended::TimedOut)), "{ran:?}");
    assert!(seconds < 5.0, "returned after {seconds} s");
    let grandchild = std::fs::read_to_string(dir.join("grandchild")).expect("sh wrote the pid");
    assert!(
        has_ended(grandchild.trim()),
        "the grandchild {grandchild} still runs"
    );
}

#[cfg(unix)]
#[test]
fn an_inherited_command_past_its_timeout_is_killed() {
    let cmd = Cmd::new(Program::Named("sleep"), ["30"]);
    let (ran, seconds) = crate::clock::timed(|| {
        run_timed(
            &cmd,
            Path::new("."),
            Output::Inherit,
            Duration::from_millis(500),
        )
    });
    assert!(matches!(ran, Ok(Ended::TimedOut)), "{ran:?}");
    assert!(seconds < 5.0, "returned after {seconds} s");
}
