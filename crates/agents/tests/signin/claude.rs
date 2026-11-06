//! Claude's sign-in on the fake CLI, end to end through `Agents`: the run that completes, the
//! nudge, Cancel, expiry, the paste route, the exits before sign-in, `--console`, sign-out, the
//! calls refused, and no status run after the end.

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, Method, PasteCode, SigninCode, SigninError, SigninState, Source};

use crate::support::{
    Events, Fixture, LOGIN_LIMIT, NUDGE, POLL, STATUS_LIMIT, Seen, claude, codex, now,
};

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some("test@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

fn failed(code: SigninCode) -> SigninState {
    SigninState::Failed { code, at: now() }
}

fn waiting(nudge: bool) -> SigninState {
    SigninState::Waiting {
        since: now(),
        nudge,
    }
}

/// The states among `seen`, `OpenUrl` left out.
fn states(seen: &[Seen]) -> Vec<SigninState> {
    seen.iter()
        .filter_map(|seen| match seen {
            Seen::State(state) => Some(state.clone()),
            Seen::Open(_) => None,
        })
        .collect()
}

/// Signs in and waits for `Waiting`.
fn start_waiting(fx: &Fixture, method: Method) -> (clauseo_agents::Agents, Events) {
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), method).unwrap();
    let seen = events.until(|state| matches!(state, SigninState::Waiting { .. }));
    assert_eq!(states(&seen), [waiting(false)]);
    (agents, events)
}

#[test]
fn a_sign_in_completes_with_the_account_plan_and_time() {
    let fx = Fixture::new("signin-ok", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(states(&seen), [waiting(false), signed_in()]);
    assert_eq!(agents.signin_state(&claude()), Some(signed_in()));
    let logins = fx.runs_of("login");
    assert_eq!(logins.len(), 1);
    assert!(logins[0].args.ends_with(&[
        "--cli".to_owned(),
        "auth".to_owned(),
        "login".to_owned(),
        "--claudeai".to_owned()
    ]));
    // Confirmed by the vendor's own status, in Clauseo's folder.
    let status = fx.runs_of("status");
    assert_ne!(status, []);
    assert!(status[0].args.ends_with(&["--json".to_owned()]));
    let home = fx.config.signin.homes[&claude()].as_os_str();
    for run in fx.launches() {
        assert_eq!(
            run.env
                .get(std::ffi::OsStr::new("CLAUDE_CONFIG_DIR"))
                .map(std::ffi::OsString::as_os_str),
            Some(home)
        );
    }
}

#[test]
fn console_runs_the_console_login() {
    let fx = Fixture::new("signin-console", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::Console).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, signed_in());
    let logins = fx.runs_of("login");
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0].args.last().map(String::as_str), Some("--console"));
}

#[test]
fn the_nudge_comes_at_30_s_and_not_before() {
    let fx = Fixture::new("signin-nudge", "never");
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    // Three polls (4.5 s) pass with no nudge; each fire waits for the poll before it to end.
    for _ in 0..3 {
        fx.timer.fire(POLL);
    }
    let early: Vec<_> = events.drain();
    assert!(
        !early
            .iter()
            .any(|event| format!("{event:?}").contains("nudge: true")),
        "{early:?}"
    );
    assert_eq!(agents.signin_state(&claude()), Some(waiting(false)));
    assert_eq!(
        fx.timer
            .armed()
            .iter()
            .filter(|wait| **wait == NUDGE)
            .count(),
        1
    );
    fx.timer.fire(NUDGE);
    let seen = events.until(|state| *state == waiting(true));
    assert_eq!(states(&seen), [waiting(true)]);
    agents.cancel_sign_in(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Cancelled));
}

#[test]
fn cancel_kills_the_run_and_its_tree() {
    let fx = Fixture::new("signin-cancel", "never");
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    crate::support::eventually("the login started", || !fx.runs_of("login").is_empty());
    let login = fx.runs_of("login").remove(0);
    agents.cancel_sign_in(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Cancelled));
    #[cfg(target_os = "linux")]
    assert!(
        crate::support::gone(login.pid),
        "pid {} still runs",
        login.pid
    );
    let _ = login;
    // Nothing is left to run: no poll armed, and Cancel has nothing to stop.
    assert_eq!(fx.timer.live(), Vec::<std::time::Duration>::new());
    assert_eq!(fx.runs_of("status"), []);
    assert_eq!(
        agents.cancel_sign_in(&claude()),
        Err(SigninError::NotRunning(claude()))
    );
}

#[test]
fn expiry_at_10_minutes_gives_link_expired_with_its_time() {
    let fx = Fixture::new("signin-expiry", "never");
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    fx.timer.fire(LOGIN_LIMIT);
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::LinkExpired));
    assert_eq!(Some(last), agents.signin_state(&claude()));
    assert_eq!(fx.timer.live(), Vec::<std::time::Duration>::new());
    #[cfg(target_os = "linux")]
    {
        let login = fx.runs_of("login").remove(0);
        assert!(crate::support::gone(login.pid));
    }
}

#[test]
fn a_pasted_code_reaches_stdin_and_shows_nowhere() {
    const CODE: &str = "FAKE-CODE";
    let fx = Fixture::new("signin-paste", "paste");
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    agents
        .paste_code(&claude(), PasteCode::from(CODE.to_owned()))
        .unwrap();
    // The fake signs in only on that exact line from its stdin.
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    let refused = agents
        .paste_code(&claude(), PasteCode::from(CODE.to_owned()))
        .unwrap_err();
    let mut printed = vec![
        format!("{seen:?}"),
        format!("{:?}", events.drain()),
        format!("{agents:?}"),
        format!("{:?}", agents.signin_state(&claude())),
        format!("{refused:?}"),
        refused.to_string(),
        format!("{:?}", PasteCode::from(CODE.to_owned())),
    ];
    for run in fx.launches() {
        printed.push(format!("{:?} {:?}", run.args, run.env));
    }
    for text in printed {
        assert!(!text.contains(CODE), "{text}");
    }
}

#[test]
fn an_exit_before_sign_in_fails() {
    let fx = Fixture::new("signin-exit", "paste");
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    // A refused code: the fake prints that it failed and exits 1.
    agents
        .paste_code(&claude(), PasteCode::from("WRONG-0000".to_owned()))
        .unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Failed));
}

#[test]
fn a_refusal_on_the_page_is_cancelled() {
    let fx = Fixture::new("signin-denied", "cancel");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::Cancelled));
}

#[test]
fn sign_out_runs_the_logout_then_status() {
    let fx = Fixture::new("signin-out", "ok-after:0");
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    agents.sign_out(&claude()).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(states(&seen), [SigninState::SignedOut { found: None }]);
    assert_eq!(last, SigninState::SignedOut { found: None });
    let logouts = fx.runs_of("logout");
    assert_eq!(logouts.len(), 1);
    assert!(logouts[0].args.ends_with(&[
        "--cli".to_owned(),
        "auth".to_owned(),
        "logout".to_owned()
    ]));
    // A status check now agrees, and finds no login of the lawyer's.
    agents.check_status(&claude()).unwrap();
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    assert_eq!(events.until_end().1, none);
}

#[test]
fn check_status_reports_a_login_already_there() {
    let fx = Fixture::new("signin-status", "never");
    let home = fx.config.signin.homes[&claude()].clone();
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"test@example.com"}"#,
    )
    .unwrap();
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    assert_eq!(fx.runs_of("login"), []);
    assert_eq!(fx.runs_of("status").len(), 1);
}

#[test]
fn no_status_runs_once_a_poll_confirms_the_sign_in() {
    let fx = Fixture::new("signin-confirmed", "never");
    // The login never ends by itself; the first poll finds the account signed in.
    let home = fx.config.signin.homes[&claude()].clone();
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"test@example.com"}"#,
    )
    .unwrap();
    let (agents, events) = start_waiting(&fx, Method::ClaudeAi);
    fx.timer.fire(POLL);
    let (_, last) = events.until_end();
    assert_eq!(last, signed_in());
    // By the time `SignedIn` is sent, the login, the polls and their limits are all gone.
    assert_eq!(fx.timer.live(), Vec::<std::time::Duration>::new());
    let runs = fx.launches().len();
    assert_eq!(fx.runs_of("status").len(), 1);
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(fx.runs_of("login")[0].pid));
    assert!(fx.timer.armed().contains(&STATUS_LIMIT));
    assert_eq!(fx.launches().len(), runs);
    assert_eq!(
        agents.cancel_sign_in(&claude()),
        Err(SigninError::NotRunning(claude()))
    );
}

#[test]
fn calls_that_cannot_run_are_refused() {
    let mut fx = Fixture::new("signin-refused", "never");
    let unknown = clauseo_protocol::AgentId::new("other-acp").unwrap();
    {
        let (agents, _events) = fx.start();
        assert_eq!(
            agents.try_again(&claude()),
            Err(SigninError::WrongMethod {
                agent: claude(),
                method: Method::Browser
            })
        );
        assert_eq!(
            agents.sign_in(&claude(), Method::DeviceCode),
            Err(SigninError::WrongMethod {
                agent: claude(),
                method: Method::DeviceCode
            })
        );
        assert_eq!(
            agents.sign_in(&codex(), Method::ClaudeAi),
            Err(SigninError::WrongMethod {
                agent: codex(),
                method: Method::ClaudeAi
            })
        );
        assert_eq!(
            agents.sign_in(&unknown, Method::ClaudeAi),
            Err(SigninError::UnknownAgent(unknown.clone()))
        );
        assert_eq!(
            agents.paste_code(&claude(), PasteCode::from("x".to_owned())),
            Err(SigninError::NotRunning(claude()))
        );
        assert_eq!(agents.signin_state(&claude()), None);
    }
    fx.config.signin.homes.clear();
    let (agents, _events) = fx.start();
    assert_eq!(
        agents.sign_in(&claude(), Method::ClaudeAi),
        Err(SigninError::NoHome(claude()))
    );
    assert_eq!(fx.launches(), []);
}

#[test]
fn sign_in_runs_through_the_sign_in_launcher_and_never_the_install_one() {
    let fx = Fixture::new("signin-split", "ok-after:0");
    let (agents, events, installs) = fx.start_split(claude());
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(
        fx.runs_of("login").len(),
        1,
        "the login went through the sign-in launcher"
    );
    assert_ne!(fx.runs_of("status"), []);
    let auth_runs: Vec<String> = installs
        .args()
        .into_iter()
        .filter(|args| args.contains("\"auth\""))
        .collect();
    assert_eq!(
        auth_runs,
        Vec::<String>::new(),
        "no sign-in run on the install launcher"
    );
}
