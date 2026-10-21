//! Tests for `process.rs`: how git, pnpm and commands with a limit run. `tree.rs` tests how a
//! command's tree is stopped.

use super::*;
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
fn cargo_runs_without_clippy_conf_dir() {
    let cargo = Program::Cargo.command();
    let removed: Vec<&OsStr> = cargo
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name)
        .collect();
    assert_eq!(removed, [OsStr::new(CLIPPY_CONF_DIR)]);
}

#[test]
fn cmd_displays_its_program_then_its_args() {
    let cmd = Cmd::new(
        Program::Cargo,
        ["fmt", "--all", "--", "--check"],
        TEST_TIMEOUT,
    );
    assert_eq!(cmd.to_string(), "cargo fmt --all -- --check");
    let named = Cmd::new(Program::Named("git"), Vec::<String>::new(), TEST_TIMEOUT);
    assert_eq!(named.to_string(), "git");
    let pnpm = Cmd::new(Program::Pnpm, ["--version"], TEST_TIMEOUT);
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
fn an_empty_path_entry_does_not_find_a_pnpm_in_the_working_directory() {
    let files = [at("", "pnpm.exe"), at("/usr/local/node", "pnpm.cmd")];
    let path = path_of(&["", "/usr/local/node"]);
    assert_eq!(
        resolve_pnpm(&path, &WINDOWS_PNPM, only(&files)),
        at("/usr/local/node", "pnpm.cmd")
    );
    let only_empty = path_of(&["", "/tools/bin"]);
    assert_eq!(
        resolve_pnpm(&only_empty, &WINDOWS_PNPM, only(&files)),
        PathBuf::from("pnpm")
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
        TEST_TIMEOUT,
    );
    let mut seen = Vec::new();
    let captured = capture(&cmd, Path::new("."), &mut |chunk| {
        seen.extend_from_slice(chunk);
    })
    .expect("sh runs");
    assert!(!captured.success());
    assert!(captured.stdout.contains("out"));
    assert!(captured.stdout.contains("err"));
    assert_eq!(seen, captured.stdout.as_bytes());
}

/// The names in `listed`, the output of `git rev-parse --local-env-vars`, that `GIT_LOCAL_ENV`
/// does not hold.
fn missing_from_git_local_env(listed: &str) -> Vec<&str> {
    listed
        .lines()
        .filter(|name| !GIT_LOCAL_ENV.contains(name))
        .collect()
}

#[test]
fn every_variable_the_local_git_lists_is_in_git_local_env() {
    let cmd = Cmd::new(
        Program::Git,
        ["rev-parse", "--local-env-vars"],
        TEST_TIMEOUT,
    );
    let listed = run_quiet(&cmd, Path::new(".")).expect("git runs");
    assert!(listed.success(), "`{cmd}` failed: {}", listed.stderr);
    assert_eq!(
        missing_from_git_local_env(&listed.stdout),
        Vec::<&str>::new(),
        "`{cmd}` lists variables GIT_LOCAL_ENV in xtask/src/process.rs lacks: add them"
    );
}

/// `git rev-parse --local-env-vars` as git 2.39 prints it, with `GIT_SUPER_PREFIX`.
const GIT_2_39_LOCAL_ENV: &str = "GIT_ALTERNATE_OBJECT_DIRECTORIES\nGIT_CONFIG\n\
    GIT_CONFIG_PARAMETERS\nGIT_CONFIG_COUNT\nGIT_OBJECT_DIRECTORY\nGIT_DIR\nGIT_WORK_TREE\n\
    GIT_IMPLICIT_WORK_TREE\nGIT_GRAFT_FILE\nGIT_INDEX_FILE\nGIT_NO_REPLACE_OBJECTS\n\
    GIT_REPLACE_REF_BASE\nGIT_PREFIX\nGIT_SUPER_PREFIX\nGIT_SHALLOW_FILE\nGIT_COMMON_DIR\n";

#[test]
fn an_older_git_list_passes_and_a_list_with_an_unknown_name_fails() {
    assert_eq!(
        missing_from_git_local_env(GIT_2_39_LOCAL_ENV),
        Vec::<&str>::new()
    );
    let newer = format!("{GIT_2_39_LOCAL_ENV}GIT_SOMETHING_NEW\n");
    assert_eq!(missing_from_git_local_env(&newer), ["GIT_SOMETHING_NEW"]);
}

#[test]
fn a_command_that_cannot_start_is_an_error() {
    let cmd = Cmd::new(
        Program::Named("xtask-no-such-program"),
        ["--version"],
        TEST_TIMEOUT,
    );
    let ran = run(&cmd, Path::new("."), Output::Quiet);
    assert_eq!(
        ran.expect_err("the program does not exist").kind(),
        io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn a_command_that_exits_gives_its_status_and_output() {
    let cmd = Cmd::new(
        Program::Named("sh"),
        ["-c", "echo out; echo err >&2; exit 3"],
        TEST_TIMEOUT,
    );
    let quiet = run(&cmd, Path::new("."), Output::Quiet).expect("sh runs");
    assert_eq!(quiet.status, Status::Failed);
    assert!(!quiet.success());
    assert_eq!(quiet.stdout, "out\n");
    assert_eq!(quiet.stderr, "err\n");
}

#[cfg(unix)]
#[test]
fn an_inherited_command_keeps_no_output() {
    let cmd = Cmd::new(Program::Named("true"), Vec::<String>::new(), TEST_TIMEOUT);
    let ran = run(&cmd, Path::new("."), Output::Inherit).expect("true runs");
    assert_eq!(ran.status, Status::Succeeded);
    assert!(ran.success());
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
}

#[test]
fn a_limit_is_written_in_its_largest_whole_unit() {
    let cases = [
        (Duration::from_hours(1), "1 h"),
        (Duration::from_mins(90), "90 min"),
        (Duration::from_secs(30), "30 s"),
        (Duration::from_secs(90), "90 s"),
        (Duration::from_millis(1500), "1500 ms"),
        (Duration::from_millis(500), "500 ms"),
    ];
    for (limit, text) in cases {
        assert_eq!(limit_text(limit), text);
    }
}

#[test]
fn a_timeout_names_the_command_and_its_limit() {
    let cmd = Cmd::new(Program::Cargo, ["nextest", "run"], Duration::from_mins(45));
    assert_eq!(
        timeout_message(&cmd),
        "`cargo nextest run` did not finish within 45 min, so it was stopped"
    );
    assert_eq!(
        TimeoutFix::of(&cmd).to_string(),
        "run `cargo nextest run` by hand to see why it does not finish within 45 min"
    );
}

#[cfg(unix)]
#[test]
fn the_restarted_leader_mode_passes_only_when_both_runs_pass() {
    let mode = |script: &str| {
        let cmd = Cmd::new(Program::Named("sh"), ["-c", script], TEST_TIMEOUT);
        test_mode::restarted_leader(&cmd, script)
    };
    assert!(mode("exit 0"));
    assert!(!mode(r#"[ "$1" != again ]"#), "the second run fails");
    assert!(!mode(r#"[ "$1" = again ]"#), "the first run fails");
}

#[cfg(unix)]
pub(super) mod tree;

#[cfg(unix)]
mod denied;
