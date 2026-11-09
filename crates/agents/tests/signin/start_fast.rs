//! The faster start-up check: Claude's detection runs alongside the status of Clauseo's own
//! login when nothing is recorded (never on macOS), and picks what the serial check picks; it
//! writes nothing the serial check would not; only the start-up check's runs get
//! `STATUS_RUN_PRIORITY_CLASS`, a sign-in's never; and a sign-out of Clauseo's own login clears
//! `login-source`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, LoginFrom, Method, STATUS_RUN_PRIORITY_CLASS, SigninState, Source};

use crate::support::{Fixture, claude, codex, now};

const EMAIL: &str = "test@example.com";

/// Clauseo's own Claude login, already there in its folder.
fn plant_own(fx: &Fixture) {
    let home = fx.config.signin.homes[&claude()].clone();
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"test@example.com"}"#,
    )
    .unwrap();
}

/// The lawyer's own Claude Code login, in `$HOME/.claude`.
fn plant_lawyer(fx: &Fixture) {
    write_file(
        &fx.user_home().join(".claude/fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"lawyer@example.com"}"#,
    )
    .unwrap();
}

/// `login-source` `own`, as a sign-in of Clauseo's own that ended signed in writes it.
fn record_own(fx: &Fixture) {
    write_file(&fx.marker(&claude(), "login-source"), b"own\n").unwrap();
}

/// Whether a run had detection's environment (no `CLAUDE_CONFIG_DIR`).
#[cfg(unix)]
fn is_detection(run: &crate::support::Launch) -> bool {
    !run.env
        .contains_key(std::ffi::OsStr::new("CLAUDE_CONFIG_DIR"))
}

/// Every file under `root` with its bytes, by its path under `root`.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = std::fs::read_dir(at) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                out.insert(path.strip_prefix(root).unwrap().to_path_buf(), bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// The folder detection runs in.
fn detect_folder(fx: &Fixture) -> PathBuf {
    fx.data.path().join("agents/signin/detect")
}

/// With nothing recorded, detection starts while the own status still runs: both status runs
/// are started though neither answers (every `status` run stalls). The serial check would
/// start detection only once the own status had answered.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn with_nothing_recorded_detection_runs_alongside_the_own_status() {
    let fx = Fixture::new("fast-alongside", "ok-after:0");
    fx.stall(Some("status"));
    let (agents, _events) = fx.start();
    agents.check_status(&claude()).unwrap();
    crate::support::eventually("both status runs started", || {
        fx.runs_of("status").len() == 2
    });
    let runs = fx.runs_of("status");
    assert_eq!(runs.iter().filter(|run| is_detection(run)).count(), 1);
}

/// With a login of Clauseo's own recorded, detection waits for the own status, as before: one
/// status run while it stalls.
#[cfg(unix)]
#[test]
fn with_an_own_login_recorded_detection_waits_for_the_own_status() {
    let fx = Fixture::new("fast-recorded", "ok-after:0");
    record_own(&fx);
    fx.stall(Some("status"));
    let (agents, _events) = fx.start();
    agents.check_status(&claude()).unwrap();
    crate::support::eventually("the own status started", || fx.runs_of("status").len() == 1);
    for _ in 0..20 {
        crate::support::pause();
    }
    let runs = fx.runs_of("status");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(!is_detection(&runs[0]));
}

/// The state the start-up check ends in, with the own login and the lawyer's as given, and
/// `login-source` `own` recorded when `serial` (so the check runs serially on every OS).
fn start_check(label: &str, own: bool, lawyer: bool, serial: bool) -> SigninState {
    let fx = Fixture::new(label, "ok-after:0");
    if own {
        plant_own(&fx);
    }
    if lawyer {
        plant_lawyer(&fx);
    }
    if serial {
        record_own(&fx);
    }
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    events.state()
}

/// The parallel check (nothing recorded) and the serial one (`own` recorded) end in the same
/// state, `expected`, with the own login signed in when `own` and the lawyer's there when
/// `lawyer`.
fn same_pick(label: &str, own: bool, lawyer: bool, expected: &SigninState) {
    let parallel = start_check(&format!("{label}-p"), own, lawyer, false);
    let serial = start_check(&format!("{label}-s"), own, lawyer, true);
    assert_eq!(parallel, serial, "parallel against serial");
    assert_eq!(&parallel, expected);
}

fn own_signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

/// Both signed in: the own login wins and detection's find is dropped.
#[test]
fn both_signed_in_the_parallel_check_picks_the_own_login_as_the_serial_one_does() {
    same_pick("fast-pick-both", true, true, &own_signed_in());
}

/// The own login alone: signed in.
#[test]
fn own_only_the_parallel_check_picks_the_own_login_as_the_serial_one_does() {
    same_pick("fast-pick-own", true, false, &own_signed_in());
}

/// The lawyer's login alone: detection's `LoginFound`.
#[test]
fn lawyer_s_only_the_parallel_check_picks_the_found_login_as_the_serial_one_does() {
    let found = SigninState::LoginFound {
        from: LoginFrom::ClaudeCode,
        email: Some("lawyer@example.com".to_owned()),
    };
    same_pick("fast-pick-lawyer", false, true, &found);
}

/// Neither: signed out with nothing found.
#[test]
fn neither_the_parallel_check_picks_signed_out_as_the_serial_one_does() {
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    same_pick("fast-pick-none", false, false, &none);
}

/// What the start-up check writes under the lawyer's home and detection's folder, with Claude's
/// status writing its config file as Claude Code's does (`FAKE_CLAUDE_STATUS_WRITES`), and
/// `login-source` as `record` says. The own login is signed in.
fn writes(label: &str, record: bool) -> (BTreeMap<PathBuf, Vec<u8>>, BTreeMap<PathBuf, Vec<u8>>) {
    let mut fx = Fixture::new(label, "ok-after:0");
    fx.config
        .signin
        .base_env
        .insert("FAKE_CLAUDE_STATUS_WRITES".into(), "1".into());
    plant_own(&fx);
    plant_lawyer(&fx);
    if record {
        record_own(&fx);
    }
    let (home, folder) = (snapshot(&fx.user_home()), snapshot(&detect_folder(&fx)));
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert!(matches!(
        events.until_end().1,
        SigninState::SignedIn {
            source: Source::Own,
            ..
        }
    ));
    let changed = |before: BTreeMap<PathBuf, Vec<u8>>, after: BTreeMap<PathBuf, Vec<u8>>| {
        after
            .into_iter()
            .filter(|(path, bytes)| before.get(path) != Some(bytes))
            .collect::<BTreeMap<_, _>>()
    };
    let home = changed(home, snapshot(&fx.user_home()));
    let folder = changed(folder, snapshot(&detect_folder(&fx)));
    (home, folder)
}

/// Detection run early writes nothing the serial check would not: with a login of Clauseo's
/// own recorded (the serial check, signed in, runs no detection) nothing changes under the
/// lawyer's home or detection's folder, though the CLI's status writes its config file when it
/// runs there.
#[test]
fn early_detection_writes_nothing_the_serial_check_would_not() {
    let (home, folder) = writes("fast-writes", true);
    assert_eq!(home, BTreeMap::new(), "the lawyer's home changed");
    assert_eq!(folder, BTreeMap::new(), "detection's folder changed");
}

/// The start-up check's status and detection runs start in `STATUS_RUN_PRIORITY_CLASS` (normal,
/// `None`, for now); a sign-in's login and its polls never get a priority class.
#[test]
fn only_the_start_up_check_s_runs_get_the_status_priority_class() {
    let fx = Fixture::new("fast-background", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert!(matches!(events.state(), SigninState::LoginFound { .. }));
    let checks = fx.launches();
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert!(
        checks
            .iter()
            .all(|run| run.priority == STATUS_RUN_PRIORITY_CLASS),
        "{checks:?}"
    );
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    events.until_end();
    let sign_in = &fx.launches()[checks.len()..];
    assert!(sign_in.iter().any(|run| run.is("login")), "{sign_in:?}");
    assert!(
        sign_in.iter().all(|run| run.priority.is_none()),
        "{sign_in:?}"
    );
}

/// Signing out of Clauseo's own Claude login clears `login-source` `own`, so no later start
/// reads a login of Clauseo's own that is gone.
#[test]
fn signing_out_of_clauseo_s_own_claude_login_clears_its_record() {
    let fx = Fixture::new("fast-out-claude", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    events.until_end();
    let source = fx.marker(&claude(), "login-source");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "own\n");
    agents.sign_out(&claude()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    assert!(!source.exists(), "login-source is still there");
}

/// Codex's equivalent: its sign-out of Clauseo's own login clears the record too.
#[test]
fn signing_out_of_clauseo_s_own_codex_login_clears_its_record() {
    let fx = Fixture::new("fast-out-codex", "ok-after:0");
    let (agents, events) = fx.start_for(codex());
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert!(matches!(
        events.until_end().1,
        SigninState::SignedIn {
            source: Source::Own,
            ..
        }
    ));
    let source = fx.marker(&codex(), "login-source");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "own\n");
    agents.sign_out(&codex()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    assert!(!source.exists(), "login-source is still there");
}
