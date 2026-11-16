//! The lawyer's Cancel that lands after Codex's sign-in started but before `codex login` is
//! spawned: the job ends `Failed { Cancelled }` with no login started and no browser opened,
//! well inside the lawyer's 2 s grace (it is never fired). The fake's `login status` holds the
//! sign-in's quick check until the test opens its gate (`FAKE_STATUS_GATE`), so the Cancel
//! lands before the spawn on every run, with no sleep. The spawn itself and the last look at
//! the stop are one step (`Cancel::unless_stopped`, pinned in `signin/run/tests.rs`); a Cancel
//! after the spawn still stops the login gracefully (`graceful.rs`'s
//! `cancel_sends_cancel_and_the_login_ends_writing_nothing`).

use clauseo_agents::testing::write_file;
use clauseo_agents::{Method, SigninCode, SigninState};

use crate::codex_cli::{home, start};
use crate::support::{Fixture, Seen, codex, eventually, now};

#[test]
fn a_cancel_before_the_spawn_starts_no_login() {
    let mut fx = Fixture::new("cancel-before-spawn", "never");
    fx.listen_for_cancel();
    let gate = fx.data.path().join("status-gate");
    fx.set_env("FAKE_STATUS_GATE", gate.to_str().unwrap());
    let (agents, events) = start(&fx);
    agents.sign_in(&codex(), Method::Browser).unwrap();
    eventually("the quick check's status runs", || {
        !fx.codex_runs(&["login", "status"]).is_empty()
    });
    agents.cancel_sign_in(&codex()).unwrap();
    // The status answers only now, after the Cancel: the job goes on toward the login.
    write_file(&gate, b"").unwrap();
    let (seen, last) = events.until_end();
    let cancelled = SigninState::Failed {
        code: SigninCode::Cancelled,
        at: now(),
    };
    assert_eq!(last, cancelled, "{seen:?}");
    let states: Vec<&SigninState> = seen
        .iter()
        .filter_map(|seen| match seen {
            Seen::State(state) => Some(state),
            Seen::Open(_) => None,
        })
        .collect();
    assert_eq!(states, [&SigninState::Checking, &cancelled]);
    assert!(
        !seen.iter().any(|seen| matches!(seen, Seen::Open(_))),
        "a browser was asked for: {seen:?}"
    );
    assert_eq!(fx.codex_runs(&["login"]), [], "codex login was spawned");
    assert_eq!(fx.codex_runs(&["login", "status"]).len(), 1);
    assert_eq!(fx.codex_runs(&["logout"]), []);
    assert!(!home(&fx).join("fake-login-listening").exists());
    assert_eq!(fx.timer.live(), Vec::<std::time::Duration>::new());
}
