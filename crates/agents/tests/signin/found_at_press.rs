//! Sign in pressed while the lawyer's own Codex login is there (D-004): the press runs
//! detection again (a stat of the dummy login file in `$HOME/.codex`, never a credential), so
//! "Use it" is offered rather than a browser opened, whatever the status check at start saw.
//! The two cases that once went straight to the browser: the login file made after the start's
//! check, and a press kept until the install's `Ready` (no status check runs for it).

use std::path::PathBuf;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, LoginFrom, Method, SigninState, Source};

use crate::support::{Events, Fixture, Seen, codex};

/// Codex's login file's name. Tests may name it; `src` names it only in `signin/reuse.rs`.
const LOGIN_FILE: &str = "auth.json";

fn plant_lawyer(fx: &Fixture) {
    let file: PathBuf = fx.user_home().join(".codex").join(LOGIN_FILE);
    write_file(&file, b"").unwrap();
}

fn found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::Codex,
        email: None,
    }
}

/// Events up to the press's last state, `LoginFound` counted as one (`Events::until_end` stops
/// at signed in, signed out or failed only), and that state.
fn until_found_or_end(events: &Events) -> (Vec<Seen>, SigninState) {
    let seen = events.until(|state| {
        matches!(
            state,
            SigninState::LoginFound { .. }
                | SigninState::SignedIn { .. }
                | SigninState::SignedOut { .. }
                | SigninState::Failed { .. }
        )
    });
    let Some(Seen::State(last)) = seen.last().cloned() else {
        unreachable!()
    };
    (seen, last)
}

fn opened(seen: &[Seen]) -> bool {
    seen.iter().any(|seen| matches!(seen, Seen::Open(_)))
}

/// The start's check saw no login; the lawyer then signs in to Codex in Terminal. Sign in now
/// offers that login, with no `codex login` run and no page opened; "Use it" then reuses it.
#[test]
fn a_login_made_after_the_start_s_check_is_offered_at_sign_in() {
    let fx = Fixture::new("press-found-late", "ok-after:0");
    let (agents, events) = fx.start_for(codex());
    agents.check_status(&codex()).unwrap();
    assert_eq!(
        events.state(),
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
    plant_lawyer(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    let (seen, last) = until_found_or_end(&events);
    assert_eq!(last, found());
    assert!(!opened(&seen));
    assert_eq!(fx.codex_runs(&["login"]), []);
    agents.use_found_login(&codex()).unwrap();
    assert!(matches!(
        events.state(),
        SigninState::SignedIn {
            source: Source::Reused,
            ..
        }
    ));
}

/// Sign in pressed before Codex is installed is kept until `Ready`, and no status check runs
/// for it; at `Ready` it offers the lawyer's login rather than opening the browser.
#[test]
fn a_press_kept_until_ready_offers_the_lawyer_s_login() {
    let mut fx = Fixture::new("press-found-queued", "ok-after:0");
    let install = fx.not_installed(&codex());
    plant_lawyer(&fx);
    let (agents, events) = fx.start_missing(codex());
    agents.sign_in(&codex(), Method::Browser).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    agents.finish_install_for_tests(&codex(), Some(install));
    let (seen, last) = until_found_or_end(&events);
    assert_eq!(last, found());
    assert!(!opened(&seen));
    assert_eq!(fx.codex_runs(&["login"]), []);
}

/// `no-reuse` there (the lawyer chose a different account, or "Stop using it here"): Sign in
/// is Clauseo's own sign-in, as before, until "Look for my Codex sign-in".
#[test]
fn with_no_reuse_sign_in_is_clauseo_s_own() {
    let fx = Fixture::new("press-found-no-reuse", "ok-after:0");
    plant_lawyer(&fx);
    write_file(&fx.marker(&codex(), "no-reuse"), b"").unwrap();
    let (agents, events) = fx.start_for(codex());
    agents.sign_in(&codex(), Method::Browser).unwrap();
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
}
