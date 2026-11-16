//! Detection's macOS Keychain query: its exact arguments, the account rule, what each end means
//! (on every OS), and the run itself against a fake `security` (Unix) and Apple's (macOS).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::time::Duration;

use super::{
    CLAUDE_KEYCHAIN_SERVICE, KEYCHAIN_LIMIT, KeychainProbe, KeychainSeen, claude_account,
    keychain_args, keychain_seen,
};

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn strings(args: &[OsString]) -> Vec<&str> {
    args.iter().map(|arg| arg.to_str().unwrap()).collect()
}

/// The whole argument list, pinned: attributes only, so no `-w`, `-g` or `-d` can be in it.
#[test]
fn the_query_s_arguments_are_exactly_these() {
    assert_eq!(
        CLAUDE_KEYCHAIN_SERVICE,
        concat!("Claude Code", "-credentials")
    );
    let service = CLAUDE_KEYCHAIN_SERVICE;
    assert_eq!(
        strings(&keychain_args(service, Some(OsStr::new("lawyer")))),
        ["find-generic-password", "-s", service, "-a", "lawyer"]
    );
    assert_eq!(
        strings(&keychain_args(service, None)),
        ["find-generic-password", "-s", service]
    );
    assert_eq!(KEYCHAIN_LIMIT, Duration::from_secs(3));
}

/// Apple's `security` by absolute path, never a `PATH` lookup.
#[cfg(unix)]
#[test]
fn the_query_runs_apple_s_security_by_absolute_path() {
    assert_eq!(super::SECURITY, "/usr/bin/security");
    assert!(std::path::Path::new(super::SECURITY).is_absolute());
}

/// `-a` only when `USER` is Claude Code's account: `[A-Za-z0-9._-]+`.
#[test]
fn the_account_is_user_only_when_claude_would_take_it() {
    for user in ["lawyer", "j.doe_1-x", "R2"] {
        assert_eq!(
            claude_account(&env(&[("USER", user)])),
            Some(OsStr::new(user))
        );
    }
    for user in ["", "jane doe", "jüri", "a/b"] {
        assert_eq!(claude_account(&env(&[("USER", user)])), None, "{user:?}");
    }
    assert_eq!(claude_account(&env(&[("HOME", "/x")])), None);
}

/// Exit 0 is found and 44 ("could not be found") absent; any other code, a signal, a timeout
/// and a failed start could not tell.
#[test]
fn exit_0_is_found_44_absent_and_anything_else_unknown() {
    assert_eq!(
        keychain_seen(KeychainProbe::Exited(Some(0))),
        KeychainSeen::Found
    );
    assert_eq!(
        keychain_seen(KeychainProbe::Exited(Some(44))),
        KeychainSeen::Absent
    );
    for probe in [
        KeychainProbe::Exited(Some(1)),
        KeychainProbe::Exited(Some(36)),
        KeychainProbe::Exited(None),
        KeychainProbe::TimedOut,
        KeychainProbe::NotStarted,
    ] {
        assert_eq!(keychain_seen(probe), KeychainSeen::Unknown, "{probe:?}");
    }
}

#[cfg(unix)]
mod run {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions, Timer as _};
    use futures::executor::block_on;

    use super::super::{
        CLAUDE_KEYCHAIN_SERVICE, KEYCHAIN_LIMIT, KeychainProbe, KeychainSeen, claude_keychain,
        query,
    };
    use super::env;
    use crate::write::{TestDir, set_mode};

    fn launcher() -> PassthroughLauncher {
        PassthroughLauncher::new(SpawnOptions::new().clear_env())
    }

    /// A fake `security` running `body` after it records its arguments and environment.
    fn fake(dir: &TestDir, body: &str) -> PathBuf {
        let record = dir.0.join("called");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nenv > '{}.env'\n{body}\n",
            record.display(),
            record.display()
        );
        let path = dir.file("security", script.as_bytes());
        set_mode(&path, 0o755).unwrap();
        path
    }

    fn seen(program: &Path) -> KeychainSeen {
        let base = env(&[
            ("HOME", "/home/lawyer"),
            ("USER", "lawyer"),
            ("SECRET_TOKEN", "not-for-security"),
        ]);
        block_on(claude_keychain(&launcher(), &AsyncIoTimer, program, &base))
    }

    #[test]
    fn exit_0_is_found_with_the_exact_arguments_and_only_home() {
        let dir = TestDir::new("keychain-found");
        let program = fake(&dir, "echo 'acct=lawyer'; echo 'attrs' >&2; exit 0");
        assert_eq!(seen(&program), KeychainSeen::Found);
        let args = std::fs::read_to_string(dir.0.join("called")).unwrap();
        let want = format!("find-generic-password\n-s\n{CLAUDE_KEYCHAIN_SERVICE}\n-a\nlawyer\n");
        assert_eq!(args, want);
        let child_env = std::fs::read_to_string(dir.0.join("called.env")).unwrap();
        assert!(child_env.contains("HOME=/home/lawyer"), "{child_env}");
        assert!(!child_env.contains("SECRET_TOKEN"), "{child_env}");
        assert!(!child_env.contains("USER="), "{child_env}");
    }

    #[test]
    fn exit_44_is_absent_and_exit_1_unknown() {
        for (code, want) in [(44, KeychainSeen::Absent), (1, KeychainSeen::Unknown)] {
            let dir = TestDir::new("keychain-absent");
            let program = fake(&dir, &format!("exit {code}"));
            assert_eq!(seen(&program), want, "exit {code}");
            assert!(dir.0.join("called").exists(), "exit {code}");
        }
    }

    #[test]
    fn a_program_that_cannot_start_is_not_found() {
        let dir = TestDir::new("keychain-missing");
        let missing = dir.0.join("no-such-security");
        assert_eq!(
            block_on(query::run(
                &launcher(),
                &AsyncIoTimer,
                &missing,
                Vec::new(),
                &env(&[]),
                KEYCHAIN_LIMIT,
            )),
            KeychainProbe::NotStarted
        );
        assert_eq!(seen(&missing), KeychainSeen::Unknown);
    }

    /// Past its limit the query is not found and its process is killed.
    #[test]
    fn a_query_past_its_limit_is_killed_and_not_found() {
        let dir = TestDir::new("keychain-slow");
        let pid_file = dir.0.join("pid");
        let program = fake(
            &dir,
            &format!("echo $$ > '{}'\nexec sleep 30", pid_file.display()),
        );
        let probe = block_on(query::run(
            &launcher(),
            &AsyncIoTimer,
            &program,
            Vec::new(),
            &env(&[]),
            Duration::from_millis(500),
        ));
        assert_eq!(probe, KeychainProbe::TimedOut);
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid: i32 = pid.trim().parse().unwrap();
        assert!(gone(pid), "the query's process {pid} outlived its limit");
    }

    /// Whether `pid` has ended (gone, or a zombie not yet reaped) within 5 s, by `/proc` on
    /// Linux; elsewhere the handle's drop is trusted (its kill is `clauseo_process`'s own test).
    fn gone(pid: i32) -> bool {
        if !cfg!(target_os = "linux") {
            return true;
        }
        for _ in 0..100 {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // The state follows the command's closing parenthesis: `Z` is a zombie.
            let state = stat
                .rsplit_once(") ")
                .and_then(|(_, rest)| rest.chars().next());
            if matches!(state, None | Some('Z' | 'X')) {
                return true;
            }
            block_on(AsyncIoTimer.after(Duration::from_millis(50)));
        }
        false
    }
}

/// Apple's own `security`, on the Mac: a service that cannot exist exits 44, absent. The one
/// test that asks the machine's Keychain, so it opts in (`allow_real_security`).
#[cfg(target_os = "macos")]
#[test]
fn apple_s_security_says_44_for_a_missing_item() {
    use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};

    // Apple's `security` can raise the unlock dialog on a locked login keychain: skip then.
    if crate::login_keychain_locked() {
        return;
    }

    let mut signin = crate::config::SigninConfig::from_env();
    signin.allow_real_security = true;
    let program = signin.security_program();
    assert_eq!(program, std::path::Path::new(super::SECURITY));
    let service = format!("{CLAUDE_KEYCHAIN_SERVICE}-clauseo-test-absent");
    let home = std::env::var_os("HOME").unwrap();
    let base = BTreeMap::from([(OsString::from("HOME"), home)]);
    let probe = futures::executor::block_on(super::query::run(
        &PassthroughLauncher::new(SpawnOptions::new().clear_env()),
        &AsyncIoTimer,
        &program,
        keychain_args(&service, None),
        &base,
        KEYCHAIN_LIMIT,
    ));
    assert_eq!(probe, KeychainProbe::Exited(Some(44)));
    assert_eq!(keychain_seen(probe), KeychainSeen::Absent);
}

/// With no fake and no opt-in, a test that reaches the query panics instead of asking the
/// machine's Keychain.
#[test]
#[should_panic(expected = "a test reached the real Keychain query")]
fn a_test_without_a_fake_never_reaches_the_real_security() {
    drop(crate::config::SigninConfig::from_env().security_program());
}
