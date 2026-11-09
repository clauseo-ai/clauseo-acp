//! Reusing the lawyer's Codex login on the fake CLI, end to end through `Agents`: done-when row
//! 1's "Reuse, on the fakes" items for Codex, and the public split check and guard. The lawyer's
//! login is a dummy, empty login file in `$HOME/.codex` (the fixture's `HOME` is
//! `<data>/user-home`), which the fake takes as its marker; it is never a real credential.
//! Detection only stats that file (no Codex run), so `LoginFound` has no email; "Use it"
//! confirms the account through the link, which gives the email and plan.

use std::ffi::OsStr;
use std::path::PathBuf;

use clauseo_agents::testing::{remove_file, write_file};
use clauseo_agents::{
    CodexLink, Found, LoginFrom, Method, SigninCode, SigninError, SigninNote, SigninState, Source,
    codex_link_holds, codex_login_shared, same_file, unlink_reused_login,
};

use crate::support::{Events, Fixture, Launch, codex, now};
#[cfg(unix)]
use crate::support::{claude, eventually};

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

/// Detection's `LoginFound` for Codex: a stat gives no email.
fn found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::Codex,
        email: None,
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

/// Codex's detection starts no Codex run at all (its `login status` writes `~/.codex/tmp`):
/// every Codex run of a status check that detects has Clauseo's `CODEX_HOME` and the store
/// argument, though the base environment sets `CLAUDE_CONFIG_DIR` (`CODEX_HOME` there is the
/// lawyer's own, `detect_nowrite.rs`).
#[test]
fn codex_detection_starts_no_codex_run() {
    let mut fx = Fixture::new("reuse-codex-env", "ok-after:0");
    fx.config.signin.base_env.insert(
        "CLAUDE_CONFIG_DIR".into(),
        fx.data.path().join("stray").into_os_string(),
    );
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let runs = fx.launches();
    assert_ne!(runs.len(), 0);
    for run in &runs {
        assert!(has_store(run), "{:?}", run.args);
        assert_eq!(
            run.env.get(OsStr::new("CODEX_HOME")),
            Some(&home(&fx).into_os_string()),
            "{:?}",
            run.args
        );
    }
}

/// The lawyer's `~/.codex/auth.json` present (a regular file) gives `LoginFound { from: Codex,
/// email: None }` and makes no link.
#[test]
fn the_lawyer_s_file_present_gives_login_found_and_no_link() {
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

/// With no login file, nothing is found, though Codex's CLI would say signed in (the fake's
/// marker in `~/.codex`, a keyring login say): telling a keyring login apart needs the CLI,
/// which detection never runs.
#[test]
fn with_the_file_absent_codex_finds_no_login() {
    let fx = Fixture::new("reuse-codex-keyring", "ok-after:0");
    write_file(&fx.user_home().join(".codex/fake-signin-marker"), b"{}").unwrap();
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::NoLogin)));
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

/// "a failed link falls back to the sign-in run": another plain file stands where the link
/// goes, so `hard_link` fails; the `codex_link_failed` note, then Clauseo's own browser sign-in,
/// which the guard lets run (two plain files, two file ids).
#[test]
fn a_failed_link_falls_back_to_the_sign_in_run() {
    let fx = Fixture::new("reuse-codex-link-fails", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    write_file(&home_file(&fx), b"").unwrap();
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
        // Under `own` the switch guard refuses first; past it, the shared-file guard still holds.
        let refused = Err(SigninError::OwnLoginPresent(codex()));
        assert!(!own || agents.sign_in(&codex(), Method::Browser) == refused);
        agents.sign_in_switching(&codex(), Method::Browser).unwrap();
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

/// "Codex runs get `HOME` set to Clauseo's folder on macOS and Linux, Claude runs never":
/// every Codex run has the store argument and `HOME` its home (detection starts none); every
/// Claude run has the lawyer's.
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
    assert!(seen.0 > 0 && seen.1 == 0 && seen.2 > 0, "{seen:?}");
}

/// The lawyer's file-mode login whose `account/read` names no email (a `ChatGPT` account with
/// `email: null`, by the fake's dummy marker).
fn plant_lawyer_without_email(fx: &Fixture) {
    write_file(&lawyer_file(fx), br#"{"authMethod": "chatgpt-no-email"}"#).unwrap();
}

/// Whether the home holds no name of Codex's login file and no `reused` record.
fn nothing_reused(fx: &Fixture) -> bool {
    let source = std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap_or_default();
    std::fs::symlink_metadata(home_file(fx)).is_err() && !source.starts_with("reused")
}

/// Waits until `count` app-servers have started.
#[cfg(unix)]
fn until_app_servers(fx: &Fixture, count: usize) {
    eventually("an app-server started", || {
        fx.codex_runs(&["app-server"]).len() >= count
    });
}

/// S18 security review, Medium 1, first repro: "Use it" whose confirmation's `account/read`
/// (through the new link) is cut short by a cancel: the job ends `signin.cancelled`, the link
/// is removed again and no `reused` record is written.
#[cfg(unix)]
#[test]
fn cancelling_use_it_during_its_account_read_makes_no_link() {
    let fx = Fixture::new("reuse-codex-cancel-race", "ok-after:0");
    plant_lawyer_without_email(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let before = fx.codex_runs(&["app-server"]).len();
    fx.stall(Some("app-server"));
    agents.use_found_login(&codex()).unwrap();
    until_app_servers(&fx, before + 1);
    agents.cancel_sign_in(&codex()).unwrap();
    // The stalled app-server ignores its closed stdin: killed when the grace passes.
    fx.timer.fire(crate::codex::CANCEL_GRACE);
    let (_, last) = events.until_end();
    assert_eq!(
        last,
        SigninState::Failed {
            code: SigninCode::Cancelled,
            at: now()
        }
    );
    assert!(nothing_reused(&fx));
    assert_eq!(agents.signin_state(&codex()), Some(last));
}

/// S18 security review, Medium 1, second repro: `use_different_account` while "Use it" waits on
/// its confirmation's `account/read`. "Use it" is replaced and links nothing; Clauseo's own
/// sign-in runs and ends signed in, with no link and no `reused` record, and the next start
/// keeps it.
#[cfg(unix)]
#[test]
fn use_different_account_racing_use_it_makes_no_link() {
    let fx = Fixture::new("reuse-codex-different-race", "ok-after:0");
    plant_lawyer_without_email(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let before = fx.codex_runs(&["app-server"]).len();
    fx.stall(Some("app-server"));
    agents.use_found_login(&codex()).unwrap();
    until_app_servers(&fx, before + 1);
    // "Use it"'s app-server is stalled already; the sign-in's own may answer.
    fx.stall(None);
    agents.use_different_account(&codex()).unwrap();
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
    assert_eq!(fx.codex_runs(&["login"]).len(), 1);
    assert!(nothing_reused(&fx));
    drop(agents);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert!(
        matches!(
            events.state(),
            SigninState::SignedIn {
                source: Source::Own,
                ..
            }
        ),
        "the lawyer's choice holds"
    );
    assert!(nothing_reused(&fx));
}

/// A cancel during "Use it"'s confirmation of a login with an email ends the job
/// `signin.cancelled` (not `signin.failed`), with nothing reused.
#[cfg(unix)]
#[test]
fn cancelling_use_it_during_its_confirmation_gives_cancelled() {
    let fx = Fixture::new("reuse-codex-cancel-detect", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let before = fx.codex_runs(&["app-server"]).len();
    fx.stall(Some("app-server"));
    agents.use_found_login(&codex()).unwrap();
    until_app_servers(&fx, before + 1);
    agents.cancel_sign_in(&codex()).unwrap();
    fx.timer.fire(crate::codex::CANCEL_GRACE);
    assert_eq!(
        events.state(),
        SigninState::Failed {
            code: SigninCode::Cancelled,
            at: now()
        }
    );
    assert!(nothing_reused(&fx));
}

/// "Use it" whose confirmation never answers (the app-server killed at its limit) is
/// `signin.failed`: the link is removed again and nothing is recorded, so the lawyer may try
/// again.
#[cfg(target_os = "linux")]
#[test]
fn use_it_whose_confirmation_never_answers_fails_and_reuses_nothing() {
    let fx = Fixture::new("reuse-codex-unanswered", "ok-after:0");
    plant_lawyer(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    let before = fx.codex_runs(&["app-server"]).len();
    fx.stall(Some("app-server"));
    agents.use_found_login(&codex()).unwrap();
    until_app_servers(&fx, before + 1);
    fx.timer.fire(std::time::Duration::from_secs(20));
    let (_, last) = events.until_end();
    assert_eq!(
        last,
        SigninState::Failed {
            code: SigninCode::Failed,
            at: now()
        }
    );
    let app_server = fx.codex_runs(&["app-server"]).pop().unwrap();
    assert!(crate::support::gone(app_server.pid));
    assert!(nothing_reused(&fx));
    assert!(lawyer_file(&fx).is_file());
}

/// S18 security review, Low 2: the guard refuses when the home's login file is a symbolic link
/// (to the lawyer's file, which Codex would read through it), and on any error other than a
/// missing file for either path.
#[cfg(unix)]
#[test]
fn the_guard_refuses_a_linked_home_file_and_any_stat_error() {
    let fx = Fixture::new("reuse-codex-guard-link", "ok-after:0");
    plant_lawyer(&fx);
    write_file(&home(&fx).join("keep"), b"").unwrap();
    // A symbolic link to the lawyer's file.
    clauseo_agents::testing::symlink(&lawyer_file(&fx), &home_file(&fx)).unwrap();
    assert!(codex_login_shared(&home(&fx), &fx.user_home()));
    remove_file(&home_file(&fx)).unwrap();
    // The home's own plain file, another file than the lawyer's: the run may go ahead.
    write_file(&home_file(&fx), b"").unwrap();
    assert!(!codex_login_shared(&home(&fx), &fx.user_home()));
    // The home's folder is a file: the stat fails with "not a folder", not "missing".
    let blocked = fx.data.path().join("blocked-home");
    write_file(&blocked, b"").unwrap();
    assert!(codex_login_shared(&blocked, &fx.user_home()));
    // The lawyer's `.codex` is a file: their path's stat fails the same way.
    let lawyer = fx.data.path().join("blocked-lawyer");
    write_file(&lawyer.join(".codex"), b"").unwrap();
    assert!(codex_login_shared(&home(&fx), &lawyer));
    // A home with no file: the run may go ahead.
    remove_file(&home_file(&fx)).unwrap();
    assert!(!codex_login_shared(&home(&fx), &fx.user_home()));
}

/// S18 re-review, Medium: a `ChatGPT` login whose `account/read` answers with no email is
/// shown with `email: None` and "Use it" reuses it: `None` re-detected equals `None` shown.
#[test]
fn use_it_reuses_a_login_with_no_email() {
    let fx = Fixture::new("reuse-codex-no-email", "ok-after:0");
    plant_lawyer_without_email(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_found_login(&codex()).unwrap();
    let last = events.state();
    assert_eq!(
        last,
        SigninState::SignedIn {
            email: None,
            plan: Some("plus".to_owned()),
            source: Source::Reused,
            at: now(),
        }
    );
    assert!(same_file(&home_file(&fx), &lawyer_file(&fx)).unwrap());
    assert_eq!(
        std::fs::read_to_string(fx.marker(&codex(), "login-source")).unwrap(),
        "reused\n"
    );
    assert_eq!(fx.codex_runs(&["login"]), []);
}

/// An API-key login file is offered (a stat cannot tell it from a `ChatGPT` one), and "Use it"'s
/// confirmation turns it down: `SignedOut { found: ReusedSignedOut }`, the link removed again,
/// nothing recorded, no `no-reuse`, and the lawyer's file kept.
#[test]
fn use_it_turns_down_an_api_key_login() {
    let fx = Fixture::new("reuse-codex-api-key", "ok-after:0");
    write_file(&lawyer_file(&fx), br#"{"authMethod": "apikey"}"#).unwrap();
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.state(), found());
    agents.use_found_login(&codex()).unwrap();
    assert_eq!(events.state(), signed_out(Some(Found::ReusedSignedOut)));
    assert!(nothing_reused(&fx));
    assert!(!fx.marker(&codex(), "no-reuse").exists());
    assert!(lawyer_file(&fx).is_file());
    assert_eq!(fx.codex_runs(&["login"]), []);
}
