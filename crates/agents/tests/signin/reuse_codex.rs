//! Reusing the lawyer's Codex login on the fake CLI, end to end through `Agents`: done-when row
//! 1's "Reuse, on the fakes" items for Codex, and the public split check and guard. The lawyer's
//! login is a dummy, empty login file in `$HOME/.codex` (the fixture's `HOME` is
//! `<data>/user-home`), which the fake takes as its marker; it is never a real credential.

use std::ffi::OsStr;
use std::path::PathBuf;

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{
    CodexLink, Found, LoginFrom, Method, SigninCode, SigninError, SigninNote, SigninState, Source,
    codex_link_holds, codex_login_shared, same_file, unlink_reused_login,
};

use crate::support::{Events, Fixture, Launch, claude, codex, now};

/// Codex's login file's name. Tests may name it; `src` names it only in `signin/reuse.rs`.
const LOGIN_FILE: &str = "auth.json";
const STORE: &str = "cli_auth_credentials_store=\"file\"";
const EMAIL: &str = "test@example.com";

fn lawyer_file(fx: &Fixture) -> PathBuf {
    fx.user_home().join(".codex").join(LOGIN_FILE)
}

fn home(fx: &Fixture) -> PathBuf {
    fx.config.signin.homes[&codex()].clone()
}

fn home_file(fx: &Fixture) -> PathBuf {
    home(fx).join(LOGIN_FILE)
}

/// The lawyer's own file-mode Codex login: an empty dummy file.
fn plant_lawyer(fx: &Fixture) {
    write_file(&lawyer_file(fx), b"").unwrap();
}

fn found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::Codex,
        email: Some(EMAIL.to_owned()),
    }
}

fn reused() -> SigninState {
    SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Reused,
        at: now(),
    }
}

fn signed_out(found: Option<Found>) -> SigninState {
    SigninState::SignedOut { found }
}

fn start(fx: &Fixture) -> (clauseo_agents::Agents, Events) {
    fx.start_for(codex())
}

/// `check_status`, `LoginFound`, `use_found_login`, `SignedIn { Reused }`.
fn reuse(agents: &clauseo_agents::Agents, events: &Events) {
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_found_login(&codex()).unwrap();
    assert_eq!(events.state(), reused());
}

/// Whether a Codex run had the store argument: every Codex run but detection.
fn has_store(run: &Launch) -> bool {
    run.args.iter().any(|arg| arg == STORE)
}

fn logouts(fx: &Fixture) -> Vec<Launch> {
    fx.codex_runs(&["logout"])
}

/// "detection runs with `CLAUDE_CONFIG_DIR` and `CODEX_HOME` absent from the child's env" and
/// "detection runs without `cli_auth_credentials_store`": the base environment sets both
/// variables; the fake reports the names its last run (detection's `app-server`) got.
#[test]
fn codex_detection_runs_without_codex_home_or_the_store_argument() {
    let mut fx = Fixture::new("reuse-codex-env", "ok-after:0");
    for name in ["CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        fx.config
            .signin
            .base_env
            .insert(name.into(), fx.data.path().join("stray").into_os_string());
    }
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let names = fx.env_names();
    assert!(!names.iter().any(|name| name == "CODEX_HOME"), "{names:?}");
    assert!(
        !names.iter().any(|name| name == "CLAUDE_CONFIG_DIR"),
        "{names:?}"
    );
    let detection: Vec<Launch> = fx
        .launches()
        .into_iter()
        .filter(|run| run.env.get(OsStr::new("HOME")) == Some(&fx.user_home().into()))
        .collect();
    let commands: Vec<Vec<&str>> = detection.iter().map(Launch::command).collect();
    assert_eq!(commands, [vec!["login", "status"], vec!["app-server"]]);
    for run in &detection {
        assert!(!has_store(run), "{:?}", run.args);
        assert!(!run.env.contains_key(OsStr::new("CODEX_HOME")));
    }
}

/// "a Codex status of 0 with `~/.codex/auth.json` present gives `LoginFound { from: Codex,
/// email }` (the fake's `account/read`) and makes no link".
#[test]
fn a_codex_status_of_0_with_the_file_present_gives_login_found_and_no_link() {
    let fx = Fixture::new("reuse-codex-found", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    assert!(!home_file(&fx).exists());
    assert!(!fx.marker(&codex(), "login-source").exists());
    assert_eq!(fx.codex_runs(&["login"]), []);
}

/// "`use_found_login` then leaves one file under two names (same file id)", and the next start
/// gives `Reused` without asking.
#[test]
fn use_found_login_leaves_one_file_under_two_names() {
    let fx = Fixture::new("reuse-codex-link", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    assert!(same_file(&home_file(&fx), &lawyer_file(&fx)).unwrap());
    assert_eq!(
        codex_link_holds(&home(&fx), &fx.user_home()),
        CodexLink::Holds
    );
    assert_eq!(
        std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap(),
        format!("reused\n{EMAIL}\n")
    );
    assert_eq!(fx.codex_runs(&["login"]), []);
    drop(agents);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), reused());
}

/// "… and with it absent gives `found: CodexKeyring`": status 0 (the fake's marker in
/// `~/.codex`) with no login file.
#[test]
fn with_the_file_absent_codex_gives_codex_keyring() {
    let fx = Fixture::new("reuse-codex-keyring", "ok-after:0");
    write_file(&fx.user_home().join(".codex/fake-signin-marker"), b"{}").unwrap();
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::CodexKeyring)));
    assert_eq!(
        agents.use_found_login(&codex()),
        Err(SigninError::NothingFound(codex()))
    );
}

/// "`stop_using_login` with a reason clears the reuse record without writing `no-reuse`, so the
/// next start gives `LoginFound` again". The home's link goes (the leave order's unlink step);
/// the lawyer's file stays, and no logout runs.
#[test]
fn stop_using_login_with_a_reason_clears_the_record_without_no_reuse() {
    let fx = Fixture::new("reuse-codex-stop", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    agents
        .stop_using_login(&codex(), Some(Found::ReusedFailed))
        .unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedFailed)));
    assert!(!fx.marker(&codex(), "login-source").exists());
    assert!(!fx.marker(&codex(), "no-reuse").exists());
    assert!(!home_file(&fx).exists());
    assert!(lawyer_file(&fx).is_file());
    assert_eq!(logouts(&fx), []);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
}

/// "a failed link falls back to the sign-in run": a folder stands where the link goes, so
/// `hard_link` fails; the `codex_link_failed` note, then Clauseo's own browser sign-in.
#[test]
fn a_failed_link_falls_back_to_the_sign_in_run() {
    let fx = Fixture::new("reuse-codex-link-fails", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    write_file(&home_file(&fx).join("in-the-way"), b"").unwrap();
    agents.use_found_login(&codex()).unwrap();
    let (_, last) = events.until_end();
    assert!(
        matches!(
            last,
            SigninState::SignedIn {
                source: Source::Own,
                ..
            }
        ),
        "{last:?}"
    );
    assert_eq!(events.notes(), [SigninNote::CodexLinkFailed]);
    assert_eq!(fx.codex_runs(&["login"]).len(), 1);
    assert_eq!(
        std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap(),
        "own\n"
    );
}

/// "no … `codex logout` ever runs while the source is `Reused`": refused by the call, and by the
/// job for a fresh `Agents`, which then reports the reused login.
#[test]
fn no_codex_logout_runs_while_the_source_is_reused() {
    let fx = Fixture::new("reuse-codex-no-logout", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    assert_eq!(agents.sign_out(&codex()), Err(SigninError::Reused(codex())));
    drop(agents);
    let (agents, events) = start(&fx);
    agents.sign_out(&codex()).unwrap();
    assert_eq!(events.state(), reused());
    assert_eq!(logouts(&fx), []);
    assert!(lawyer_file(&fx).is_file());
}

/// "a Codex `login`, `logout` or `authenticate` is refused while the home's `auth.json` shares
/// the file id of `$HOME/.codex/auth.json`, whatever `login-source` says": `login-source` is set
/// to `own`, then removed, and the two names stay one file.
#[test]
fn a_codex_login_logout_or_authenticate_is_refused_while_the_files_share_an_id() {
    let fx = Fixture::new("reuse-codex-guard", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    drop(agents);
    for own in [true, false] {
        let source = fx.marker(&codex(), "login-source");
        if own {
            write_file(&source, b"own\n").unwrap();
        } else {
            remove_file(&source).unwrap();
        }
        // R20's `authenticate` asks the public guard.
        assert!(codex_login_shared(&home(&fx), &fx.user_home()));
        let (agents, events) = start(&fx);
        agents.sign_in(&codex(), Method::Browser).unwrap();
        assert_eq!(
            events.state(),
            SigninState::Failed {
                code: SigninCode::Failed,
                at: now()
            }
        );
        agents.sign_out(&codex()).unwrap();
        assert!(matches!(events.state(), SigninState::SignedIn { .. }));
        assert_eq!(fx.codex_runs(&["login"]), []);
        assert_eq!(logouts(&fx), []);
        assert!(same_file(&home_file(&fx), &lawyer_file(&fx)).unwrap());
    }
}

/// The split rule: once the two names are no longer one file (the lawyer's file saved anew),
/// reuse stops with `CodexLinkSplit`, the home's name goes and nothing falls back to either
/// file; with the lawyer's file gone, `ReusedSignedOut`.
#[test]
fn the_split_rule_stops_reuse_once_the_names_are_two_files() {
    let fx = Fixture::new("reuse-codex-split", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    // A rename-save: a new file under the lawyer's name.
    remove_file(&lawyer_file(&fx)).unwrap();
    plant_lawyer(&fx);
    assert_eq!(
        codex_link_holds(&home(&fx), &fx.user_home()),
        CodexLink::Split
    );
    assert_eq!(CodexLink::Split.found(), Some(Found::CodexLinkSplit));
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::CodexLinkSplit)));
    assert!(!home_file(&fx).exists());
    assert!(lawyer_file(&fx).is_file());
    // Never re-linked on its own: the next start finds the login and asks again.
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    assert!(!home_file(&fx).exists());

    agents.use_found_login(&codex()).unwrap();
    assert_eq!(events.state(), reused());
    remove_file(&lawyer_file(&fx)).unwrap();
    assert_eq!(
        codex_link_holds(&home(&fx), &fx.user_home()),
        CodexLink::Gone
    );
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedSignedOut)));
    assert!(!home_file(&fx).exists());
    assert_eq!(logouts(&fx), []);
    // The unlink step on a home with no link is fine.
    unlink_reused_login(&home(&fx)).unwrap();
}

/// "Codex runs other than detection get `HOME` set to Clauseo's folder on macOS and Linux,
/// Claude runs never": every Codex run with the store argument has `HOME` its home, every one
/// without (detection) the lawyer's; every Claude run the lawyer's.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn codex_runs_but_detection_get_clauseo_s_home_and_claude_runs_never() {
    let fx = Fixture::new("reuse-home", "ok-after:0");
    plant_lawyer(&fx);
    write_file(
        &fx.user_home().join(".claude/fake-signin-marker"),
        br#"{"authMethod":"claude.ai"}"#,
    )
    .unwrap();
    let (agents, events) = start(&fx);
    reuse(&agents, &events);
    drop(agents);
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert!(matches!(events.state(), SigninState::LoginFound { .. }));
    agents.use_found_login(&claude()).unwrap();
    assert!(matches!(events.state(), SigninState::SignedIn { .. }));
    let lawyer = fx.user_home().into_os_string();
    let codex_home = home(&fx).into_os_string();
    let mut seen = (0, 0, 0);
    for run in fx.launches() {
        let got = run.env.get(OsStr::new("HOME"));
        if run.args[0].ends_with("codex.js") {
            if has_store(&run) {
                assert_eq!(got, Some(&codex_home), "{:?}", run.args);
                seen.0 += 1;
            } else {
                assert_eq!(got, Some(&lawyer), "{:?}", run.args);
                seen.1 += 1;
            }
        } else {
            assert_eq!(got, Some(&lawyer), "{:?}", run.args);
            seen.2 += 1;
        }
    }
    assert!(seen.0 > 0 && seen.1 > 0 && seen.2 > 0, "{seen:?}");
}
