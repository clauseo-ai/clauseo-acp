//! Claude's sign-in, status and logout runs take inherited `CLAUDE_*` and `ANTHROPIC_*` names by
//! the routing rule the agent launch will import (`clauseo_process::env_routing`): a route and its
//! companions reach them, a lone companion and every account-only name never do, nor does a
//! token beside something that is no route. So a status says signed in only on Clauseo's own
//! sign-in or a firm's route: the fake CLI, like Claude Code, says signed in on an inherited
//! `CLAUDE_CODE_OAUTH_TOKEN` or `ANTHROPIC_AUTH_TOKEN` alone.

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_agents::{Found, Method, SigninState};
use clauseo_process::env_routing::{CLAUDE_ACCOUNT_ONLY, ROUTES, ROUTING_KEPT};

use crate::support::{Fixture, Launch, claude};

/// Signs in, signs out (a logout, then a status) and checks the status once more, and returns
/// the login, status and logout runs, each kind there at least once.
fn every_run(fx: &Fixture) -> Vec<(&'static str, Launch)> {
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert!(matches!(events.until_end().1, SigninState::SignedIn { .. }));
    agents.sign_out(&claude()).unwrap();
    assert_eq!(events.until_end().1, SigninState::SignedOut { found: None });
    agents.check_status(&claude()).unwrap();
    events.until_end();
    let mut runs = Vec::new();
    for word in ["login", "status", "logout"] {
        let of = fx.runs_of(word);
        assert_ne!(of, [], "no {word} run");
        runs.extend(of.into_iter().map(|run| (word, run)));
    }
    runs
}

/// `env`'s names that are `name` in any case.
fn spelled(env: &BTreeMap<OsString, OsString>, name: &str) -> Vec<String> {
    env.keys()
        .map(|key| key.to_string_lossy().into_owned())
        .filter(|key| key.eq_ignore_ascii_case(name))
        .collect()
}

#[test]
fn no_account_only_name_reaches_a_sign_in_status_or_logout_run_even_beside_a_route() {
    let mut fx = Fixture::new("signin-creds-runs", "ok-after:0");
    for name in CLAUDE_ACCOUNT_ONLY {
        fx.set_env(name, "inherited");
        fx.set_env(&name.to_ascii_lowercase(), "inherited");
    }
    fx.set_env("CLAUDE_CODE_USE_VERTEX", "1");
    for (word, run) in every_run(&fx) {
        for name in CLAUDE_ACCOUNT_ONLY {
            assert_eq!(
                spelled(&run.env, name),
                Vec::<String>::new(),
                "{name}, {word}"
            );
        }
        assert_eq!(
            spelled(&run.env, "CLAUDE_CODE_USE_VERTEX").len(),
            1,
            "{word}"
        );
    }
    // The child's own view of its environment, from its last run (the status check).
    let seen = fx.env_names();
    for name in CLAUDE_ACCOUNT_ONLY {
        assert!(
            !seen.iter().any(|key| key.eq_ignore_ascii_case(name)),
            "{name}"
        );
    }
}

#[test]
fn a_route_and_its_companions_reach_every_run_and_a_lone_companion_none() {
    let companions = &ROUTING_KEPT[ROUTES.len()..];
    // Windows reads a name in any case; macOS and Linux only as spelled.
    let model = if cfg!(windows) {
        "anthropic_model"
    } else {
        "ANTHROPIC_MODEL"
    };
    let mut routed = Fixture::new("signin-creds-routed", "ok-after:0");
    routed.set_env("ANTHROPIC_BASE_URL", "https://gw.firm.example");
    routed.set_env("ANTHROPIC_CUSTOM_HEADERS", "X-Firm: 1");
    routed.set_env(model, "claude-firm");
    for (word, run) in every_run(&routed) {
        for (name, value) in [
            ("ANTHROPIC_BASE_URL", "https://gw.firm.example"),
            ("ANTHROPIC_CUSTOM_HEADERS", "X-Firm: 1"),
            (model, "claude-firm"),
        ] {
            let got = run.env.get(&OsString::from(name));
            assert_eq!(got, Some(&OsString::from(value)), "{name}, {word}");
        }
    }
    let mut alone = Fixture::new("signin-creds-alone", "ok-after:0");
    for name in companions
        .iter()
        .filter(|name| **name != "ANTHROPIC_API_KEY")
    {
        alone.set_env(name, "inherited");
    }
    for (word, run) in every_run(&alone) {
        for name in companions
            .iter()
            .filter(|name| **name != "ANTHROPIC_API_KEY")
        {
            assert_eq!(
                spelled(&run.env, name),
                Vec::<String>::new(),
                "{name}, {word}"
            );
        }
        // The run's own blank key, never an inherited one.
        assert_eq!(
            run.env.get(&OsString::from("ANTHROPIC_API_KEY")),
            Some(&OsString::new())
        );
    }
}

/// The status answer comes from Clauseo's own stored sign-in and nothing inherited: with only an
/// inherited OAuth token, or only an inherited auth token, and no sign-in of Clauseo's, Claude
/// reads as signed out.
#[test]
fn an_inherited_oauth_or_auth_token_alone_reads_signed_out() {
    for name in ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN"] {
        let mut fx = Fixture::new("signin-creds-status", "never");
        fx.set_env(name, "inherited");
        let (agents, events) = fx.start();
        agents.check_status(&claude()).unwrap();
        let none = SigninState::SignedOut {
            found: Some(Found::NoLogin),
        };
        assert_eq!(events.until_end().1, none, "{name}");
        assert_ne!(fx.runs_of("status"), [], "{name}");
    }
}

/// Beside a route, the firm's token is the agent's credential too, so it reaches the status run,
/// which says signed in on it, as the agent launch would use it. This is also the control for the
/// test above: the fake's status does say signed in on an inherited token that reaches it.
#[test]
fn an_auth_token_beside_a_route_reaches_the_status_run() {
    let mut fx = Fixture::new("signin-creds-routed-status", "never");
    fx.set_env("ANTHROPIC_BASE_URL", "https://gw.firm.example");
    fx.set_env("ANTHROPIC_AUTH_TOKEN", "firm-token");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    assert!(matches!(events.until_end().1, SigninState::SignedIn { .. }));
    let status = fx.runs_of("status");
    let token = status[0].env.get(&OsString::from("ANTHROPIC_AUTH_TOKEN"));
    assert_eq!(token, Some(&OsString::from("firm-token")));
}

/// Starts a status check with `shape` and an inherited `ANTHROPIC_AUTH_TOKEN` and no sign-in of
/// Clauseo's: it reads signed out, and the token never reaches the status run.
fn token_beside_reads_signed_out(shape: &[(&str, &str)]) {
    let mut fx = Fixture::new("signin-creds-no-route", "never");
    for (name, value) in shape {
        fx.set_env(name, value);
    }
    fx.set_env("ANTHROPIC_AUTH_TOKEN", "someone-elses-token");
    let (agents, events) = fx.start();
    agents.check_status(&claude()).unwrap();
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    assert_eq!(events.until_end().1, none, "{shape:?}");
    let status = fx.runs_of("status");
    assert_ne!(status, [], "{shape:?}");
    for run in status {
        let token = spelled(&run.env, "ANTHROPIC_AUTH_TOKEN");
        assert_eq!(token, Vec::<String>::new(), "{shape:?}");
    }
}

/// A provider address with its switch absent or off is no route: Claude Code goes to
/// Anthropic's own API, so a token beside it is a lone token and reads signed out.
#[test]
fn an_auth_token_beside_a_provider_address_with_no_switch_on_reads_signed_out() {
    let vertex = ("ANTHROPIC_VERTEX_BASE_URL", "https://vertex.example");
    token_beside_reads_signed_out(&[vertex]);
    token_beside_reads_signed_out(&[vertex, ("CLAUDE_CODE_USE_VERTEX", "0")]);
}

/// An `ANTHROPIC_BASE_URL` of only spaces, only U+FEFF or only U+3000 is blank to Claude Code
/// (it trims JavaScript whitespace), so no route: a token beside it reads signed out.
#[test]
fn an_auth_token_beside_a_blank_base_url_reads_signed_out() {
    for blank in ["   ", "\u{FEFF}", "\u{3000}"] {
        token_beside_reads_signed_out(&[("ANTHROPIC_BASE_URL", blank)]);
    }
}

/// On macOS and Linux Claude Code reads only the upper-case name, so a lower-case
/// `anthropic_base_url` is no route: a token beside it reads signed out.
#[cfg(not(windows))]
#[test]
fn an_auth_token_beside_a_lower_case_base_url_reads_signed_out() {
    token_beside_reads_signed_out(&[("anthropic_base_url", "https://gw.firm.example")]);
}
