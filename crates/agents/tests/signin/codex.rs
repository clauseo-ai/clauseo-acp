//! Codex's sign-in on the fake CLI, end to end through `Agents`: the browser run that completes
//! with `account/read`'s account, no `OpenUrl` ever, port 1455 and "Try again", the device code
//! and its expiry, the device-code refusal, the environment, Cancel, the exits before sign-in,
//! status, sign-out, and no status run after the end.

use std::ffi::{OsStr, OsString};
use std::net::{Ipv4Addr, TcpListener};
use std::time::Duration;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Agents, Found, Method, SigninCode, SigninState, Source, is_sign_in_again};

use crate::support::{Events, Fixture, POLL, REMOTE_HINTS, Seen, codex, eventually, now};

/// "Try again"'s probe interval, the device code's life, and the store argument.
const PORT_POLL: Duration = Duration::from_secs(2);
const CODE_LIFETIME: Duration = Duration::from_mins(15);
const STORE: [&str; 2] = ["-c", "cli_auth_credentials_store=\"file\""];

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some("test@example.com".to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: now(),
    }
}

fn failed(code: SigninCode) -> SigninState {
    SigninState::Failed { code, at: now() }
}

fn waiting() -> SigninState {
    SigninState::Waiting {
        since: now(),
        nudge: false,
    }
}

fn states(seen: &[Seen]) -> Vec<SigninState> {
    seen.iter()
        .filter_map(|seen| match seen {
            Seen::State(state) => Some(state.clone()),
            Seen::Open(_) => None,
        })
        .collect()
}

fn start(fx: &Fixture) -> (Agents, Events) {
    fx.start_for(codex())
}

/// Plants the fake's marker in Codex's home: a login already there.
fn plant(fx: &Fixture) {
    let home = fx.config.signin.homes[&codex()].clone();
    write_file(
        &home.join("fake-signin-marker"),
        br#"{"authMethod":"chatgpt","email":"test@example.com"}"#,
    )
    .unwrap();
}

/// No `OpenUrl` among `seen` or in what is left of the events, for any agent.
fn assert_no_open_url(seen: &[Seen], events: &Events) {
    assert!(
        !seen.iter().any(|seen| matches!(seen, Seen::Open(_))),
        "{seen:?}"
    );
    let left = format!("{:?}", events.drain());
    assert!(!left.contains("OpenUrl"), "{left}");
}

#[test]
fn a_codex_sign_in_completes_with_the_account_and_plan_from_account_read() {
    let fx = Fixture::new("codex-ok", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(states(&seen), [waiting(), signed_in()]);
    assert_eq!(agents.signin_state(&codex()), Some(signed_in()));
    let logins = fx.codex_runs(&["login"]);
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0].args[1..3], STORE);
    // Confirmed by `login status`, then the account from `app-server`'s `account/read`.
    assert_ne!(fx.codex_runs(&["login", "status"]), []);
    let app_servers = fx.codex_runs(&["app-server"]);
    assert_eq!(app_servers.len(), 1);
    assert_eq!(app_servers[0].args[1..3], STORE);
    assert!(
        app_servers[0]
            .args
            .contains(&"features.plugins=false".to_owned())
    );
    // The fake printed its callback address; Codex opens its own browser, Clauseo nothing.
    assert_no_open_url(&seen, &events);
}

#[test]
fn codex_never_produces_open_url() {
    for (method, script) in [(Method::Browser, "never"), (Method::DeviceCode, "never")] {
        let fx = Fixture::new("codex-no-open", script);
        let (agents, events) = start(&fx);
        agents.sign_in(&codex(), method).unwrap();
        eventually("the login printed its lines", || {
            !fx.codex_runs(&["login"]).is_empty()
                || !fx.codex_runs(&["login", "--device-auth"]).is_empty()
        });
        // A poll (still signed out) runs while the login waits.
        fx.timer.fire(POLL);
        eventually("the poll ran", || {
            !fx.codex_runs(&["login", "status"]).is_empty()
        });
        agents.cancel_sign_in(&codex()).unwrap();
        let (seen, last) = events.until_end();
        assert_eq!(last, failed(SigninCode::Cancelled));
        assert_no_open_url(&seen, &events);
    }
}

#[test]
fn port_1455_taken_gives_port_busy_and_try_again_waits_for_it() {
    let mut fx = Fixture::new("codex-port", "ok-after:0");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    fx.config.signin.callback_port = Some(listener.local_addr().unwrap().port());
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(states(&seen), [failed(SigninCode::PortBusy)]);
    assert_eq!(last, failed(SigninCode::PortBusy));
    assert_eq!(fx.launches(), [], "no run starts while the port is taken");
    agents.try_again(&codex()).unwrap();
    let polls = || {
        fx.timer
            .armed()
            .iter()
            .filter(|wait| **wait == PORT_POLL)
            .count()
    };
    eventually("the first probe failed", || polls() == 1);
    fx.timer.fire(PORT_POLL);
    eventually("the second probe failed", || polls() == 2);
    assert_eq!(fx.launches(), []);
    assert_eq!(agents.signin_state(&codex()), Some(last));
    drop(listener);
    fx.timer.fire(PORT_POLL);
    let (seen, last) = events.until_end();
    assert_eq!(states(&seen), [waiting(), signed_in()]);
    assert_eq!(last, signed_in());
    assert_eq!(fx.codex_runs(&["login"]).len(), 1);
}

#[test]
fn try_again_is_cancelled_while_it_waits_for_the_port() {
    let mut fx = Fixture::new("codex-port-cancel", "ok-after:0");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    fx.config.signin.callback_port = Some(listener.local_addr().unwrap().port());
    let (agents, events) = start(&fx);
    agents.try_again(&codex()).unwrap();
    eventually("the probe failed", || fx.timer.live().contains(&PORT_POLL));
    agents.cancel_sign_in(&codex()).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::Cancelled));
    assert_eq!(fx.launches(), []);
    drop(listener);
}

#[test]
fn codex_s_own_port_busy_line_gives_port_busy() {
    // The probe found the port free, then Codex lost the race for it.
    let fx = Fixture::new("codex-port-race", "port-busy");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::PortBusy));
}

#[test]
fn device_code_gives_the_url_code_and_expiry_and_expires_at_15_minutes() {
    let fx = Fixture::new("codex-device", "never");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::DeviceCode).unwrap();
    let device = SigninState::DeviceCode {
        url: "https://auth.openai.com/codex/device".to_owned(),
        code: "ABCD-EFGH".to_owned(),
        expires_at: now() + CODE_LIFETIME,
    };
    let seen = events.until(|state| matches!(state, SigninState::DeviceCode { .. }));
    assert_eq!(states(&seen), [waiting(), device.clone()]);
    assert_eq!(agents.signin_state(&codex()), Some(device));
    let logins = fx.codex_runs(&["login", "--device-auth"]);
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0].args[1..3], STORE);
    fx.timer.fire(CODE_LIFETIME);
    let (_, last) = events.until_end();
    assert_eq!(last, failed(SigninCode::LinkExpired));
    assert_eq!(fx.timer.live(), Vec::<Duration>::new());
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(logins[0].pid));
}

#[test]
fn device_off_gives_device_code_off() {
    let fx = Fixture::new("codex-device-off", "device-off");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::DeviceCode).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(
        states(&seen),
        [waiting(), failed(SigninCode::DeviceCodeOff)]
    );
    assert_eq!(last, failed(SigninCode::DeviceCodeOff));
}

#[test]
fn codex_runs_get_their_home_the_file_store_and_ssl_cert_file() {
    let mut fx = Fixture::new("codex-env", "ok-after:0");
    let ca = fx.data.path().join("firm-ca.pem");
    fx.config.install.ca_certs.insert(codex(), ca.clone());
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    let home = fx.config.signin.homes[&codex()].as_os_str();
    let get = |env: &std::collections::BTreeMap<OsString, OsString>, name: &str| {
        env.get(OsStr::new(name)).cloned()
    };
    let runs = fx.launches();
    assert!(runs.len() >= 3, "{runs:?}");
    for run in runs {
        assert_eq!(run.args[1..3], STORE, "{:?}", run.args);
        assert_eq!(get(&run.env, "CODEX_HOME").as_deref(), Some(home));
        assert_eq!(
            get(&run.env, "SSL_CERT_FILE"),
            Some(ca.clone().into_os_string())
        );
        assert_eq!(get(&run.env, "CODEX_API_KEY"), Some(OsString::new()));
        assert_eq!(get(&run.env, "NODE_EXTRA_CA_CERTS"), None);
        #[cfg(unix)]
        assert_eq!(get(&run.env, "HOME").as_deref(), Some(home));
        for hint in REMOTE_HINTS {
            assert_eq!(get(&run.env, hint), None, "{hint}");
        }
    }
    // The child's own view agrees.
    let names = fx.env_names();
    assert!(names.contains(&"SSL_CERT_FILE".to_owned()), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|name| REMOTE_HINTS.contains(&name.as_str())),
        "{names:?}"
    );
}

#[test]
fn cancel_kills_the_codex_run_and_its_tree() {
    let fx = Fixture::new("codex-cancel", "never");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(
        states(&events.until(|state| *state == waiting())),
        [waiting()]
    );
    eventually("the login started", || {
        !fx.codex_runs(&["login"]).is_empty()
    });
    let login = fx.codex_runs(&["login"]).remove(0);
    agents.cancel_sign_in(&codex()).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::Cancelled));
    #[cfg(target_os = "linux")]
    assert!(
        crate::support::gone(login.pid),
        "pid {} still runs",
        login.pid
    );
    let _ = login;
    assert_eq!(fx.timer.live(), Vec::<Duration>::new());
    assert_eq!(fx.codex_runs(&["login", "status"]), []);
    assert_eq!(fx.codex_runs(&["app-server"]), []);
}

#[test]
fn a_codex_run_that_exits_before_sign_in_fails_or_is_cancelled() {
    // `paste` reads a code from a stdin that is closed: the login fails and exits 1.
    for (script, code) in [
        ("paste", SigninCode::Failed),
        ("cancel", SigninCode::Cancelled),
    ] {
        let fx = Fixture::new("codex-exit", script);
        let (agents, events) = start(&fx);
        agents.sign_in(&codex(), Method::Browser).unwrap();
        assert_eq!(events.until_end().1, failed(code), "{script}");
        assert_eq!(fx.codex_runs(&["app-server"]), []);
    }
}

#[test]
fn no_status_runs_once_a_poll_confirms_the_codex_sign_in() {
    let fx = Fixture::new("codex-confirmed", "never");
    plant(&fx);
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    events.until(|state| *state == waiting());
    fx.timer.fire(POLL);
    assert_eq!(events.until_end().1, signed_in());
    assert_eq!(fx.timer.live(), Vec::<Duration>::new());
    let runs = fx.launches().len();
    assert_eq!(fx.codex_runs(&["login", "status"]).len(), 1);
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(fx.codex_runs(&["login"])[0].pid));
    #[cfg(target_os = "linux")]
    assert!(crate::support::gone(fx.codex_runs(&["app-server"])[0].pid));
    assert_eq!(fx.launches().len(), runs);
}

#[test]
fn check_status_reads_the_account_of_a_codex_login_already_there() {
    let fx = Fixture::new("codex-status", "never");
    plant(&fx);
    let (agents, events) = start(&fx);
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    assert_eq!(fx.codex_runs(&["login"]), []);
}

#[test]
fn codex_sign_out_runs_logout_then_status() {
    let fx = Fixture::new("codex-out", "ok-after:0");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, signed_in());
    agents.sign_out(&codex()).unwrap();
    let (seen, last) = events.until_end();
    assert_eq!(states(&seen), [SigninState::SignedOut { found: None }]);
    assert_eq!(last, SigninState::SignedOut { found: None });
    let logouts = fx.codex_runs(&["logout"]);
    assert_eq!(logouts.len(), 1);
    assert_eq!(logouts[0].args[1..3], STORE);
    agents.check_status(&codex()).unwrap();
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    assert_eq!(events.until_end().1, none);
}

#[test]
fn codex_s_refresh_token_reused_classifies_as_sign_in_again() {
    // The error a turn fails with when the lawyer's CLI refreshed first (research 43).
    assert!(is_sign_in_again(
        "unexpected status 401 Unauthorized: {\"error\":{\"code\":\"refresh_token_reused\"}}"
    ));
    assert!(is_sign_in_again("Refresh_Token_Reused"));
    assert!(!is_sign_in_again("refresh_token"));
}

/// Codex's login reads no pasted code: `paste_code` is refused with `WrongMethod` (the paste
/// route is Claude's), not taken and thrown away.
#[test]
fn paste_code_to_codex_is_refused() {
    let fx = Fixture::new("codex-paste", "never");
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(
        states(&events.until(|state| *state == waiting())),
        [waiting()]
    );
    assert_eq!(
        agents.paste_code(&codex(), clauseo_agents::PasteCode::from("x".to_owned())),
        Err(clauseo_agents::SigninError::WrongMethod {
            agent: codex(),
            method: Method::ClaudeAi,
        })
    );
    agents.cancel_sign_in(&codex()).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::Cancelled));
}

/// S18 security review, drift: the agent's folders are made without following links. With
/// `<data>/agents/codex-acp` a link to another folder, the sign-in fails before any run and
/// nothing is made where the link points.
#[cfg(unix)]
#[test]
fn a_linked_agent_folder_is_refused_and_never_followed() {
    let fx = Fixture::new("codex-linked-home", "ok-after:0");
    let elsewhere = fx.data.path().join("elsewhere");
    write_file(&elsewhere.join("keep"), b"").unwrap();
    write_file(&fx.data.path().join("agents/keep"), b"").unwrap();
    clauseo_agents::testing::symlink(&elsewhere, &fx.data.path().join("agents/codex-acp")).unwrap();
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.until_end().1, failed(SigninCode::Failed));
    assert_eq!(fx.codex_runs(&["login"]), []);
    assert!(!elsewhere.join("home").exists());
}
