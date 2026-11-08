//! The queued start on the fake CLI: a sign-in pressed before the install is `Ready`, or while
//! the sign-in host does not resolve, reports `Queued` and starts by itself once both hold; it
//! stays queued through a failed install; Cancel and a new sign-in end the wait; the status
//! check needs the install but not the network.

use std::time::Duration;

use clauseo_agents::{Method, PasteCode, SigninCode, SigninError, SigninState, Source};

use crate::support::{Fixture, Seen, claude, eventually, now, pause};

/// The wait between lookups while offline: the install's.
const OFFLINE_WAIT: Duration = Duration::from_secs(30);
/// Claude's first sign-in host, the name looked up.
const HOST: &str = "claude.ai";

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: Some("test@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Own,
        at: now(),
    }
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

/// How many 30 s waits were armed so far.
fn offline_waits(fx: &Fixture) -> usize {
    fx.timer
        .armed()
        .into_iter()
        .filter(|wait| *wait == OFFLINE_WAIT)
        .count()
}

/// A few moments for a job that should stay put to have moved, if it were going to.
fn settle() {
    for _ in 0..20 {
        pause();
    }
}

#[test]
fn a_sign_in_before_the_install_is_ready_queues_then_starts_by_itself() {
    let mut fx = Fixture::new("queued-install", "ok-after:0");
    let install = fx.not_installed(&claude());
    let (agents, events) = fx.start_missing(claude());
    assert_eq!(agents.sign_in(&claude(), Method::ClaudeAi), Ok(()));
    assert_eq!(events.state(), SigninState::Queued);
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Queued));
    // No run reads a pasted code yet, and the status check still needs the install.
    assert_eq!(
        agents.paste_code(&claude(), PasteCode::from("x".to_owned())),
        Err(SigninError::NotRunning(claude()))
    );
    assert_eq!(
        agents.check_status(&claude()),
        Err(SigninError::NotReady(claude()))
    );
    settle();
    assert_eq!(fx.launches(), []);
    // Nothing is looked up before the install is ready.
    assert_eq!(fx.lookups(), Vec::<String>::new());

    agents.finish_install_for_tests(&claude(), Some(install));
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(states(&seen), [waiting(), signed_in()]);
    assert_eq!(fx.runs_of("login").len(), 1);
    assert_eq!(fx.lookups(), [HOST]);
}

#[test]
fn a_sign_in_while_offline_queues_retries_at_30_s_and_starts_once_the_host_resolves() {
    let fx = Fixture::new("queued-offline", "ok-after:0");
    fx.set_online(false);
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    eventually("the first 30 s wait", || offline_waits(&fx) == 1);
    assert_eq!(fx.timer.armed(), [OFFLINE_WAIT]);
    assert_eq!(fx.lookups(), [HOST]);

    // Still offline at 30 s: one more lookup, one more wait, no run and no second `Queued`.
    fx.timer.fire(OFFLINE_WAIT);
    eventually("the second 30 s wait", || offline_waits(&fx) == 2);
    assert_eq!(fx.lookups(), [HOST, HOST]);
    assert_eq!(fx.launches(), []);

    fx.set_online(true);
    fx.timer.fire(OFFLINE_WAIT);
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(states(&seen), [waiting(), signed_in()]);
    assert_eq!(fx.lookups(), [HOST, HOST, HOST]);
    assert_eq!(fx.runs_of("login").len(), 1);
}

#[test]
fn not_installed_and_offline_stays_queued_until_both_hold() {
    let mut fx = Fixture::new("queued-both", "ok-after:0");
    let install = fx.not_installed(&claude());
    fx.set_online(false);
    let (agents, events) = fx.start_missing(claude());
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), SigninState::Queued);

    // The install is ready, the host still does not resolve: the 30 s wait, still queued.
    agents.finish_install_for_tests(&claude(), Some(install));
    eventually("the 30 s wait", || offline_waits(&fx) == 1);
    assert_eq!(fx.lookups(), [HOST]);
    assert_eq!(fx.launches(), []);
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Queued));

    fx.set_online(true);
    fx.timer.fire(OFFLINE_WAIT);
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    // `Queued` was sent once, before the wait for the network too.
    assert_eq!(states(&seen), [waiting(), signed_in()]);
}

#[test]
fn cancel_while_queued_ends_the_wait() {
    let mut fx = Fixture::new("queued-cancel", "ok-after:0");
    let install = fx.not_installed(&claude());
    let (agents, events) = fx.start_missing(claude());
    assert!(!agents.sign_in_going(&claude()));
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert!(agents.sign_in_going(&claude()), "from the call on");
    assert_eq!(events.state(), SigninState::Queued);
    agents.cancel_sign_in(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert_eq!(
        last,
        SigninState::Failed {
            code: SigninCode::Cancelled,
            at: now()
        }
    );
    assert!(!agents.sign_in_going(&claude()), "the job ended");
    assert_eq!(
        agents.cancel_sign_in(&claude()),
        Err(SigninError::NotRunning(claude()))
    );
    // The install that comes later starts nothing.
    agents.finish_install_for_tests(&claude(), Some(install));
    settle();
    assert_eq!(fx.launches(), []);
    assert_eq!(fx.lookups(), Vec::<String>::new());

    // Offline: the cancel drops the 30 s wait too.
    fx.set_online(false);
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    eventually("the 30 s wait", || fx.timer.live() == [OFFLINE_WAIT]);
    agents.cancel_sign_in(&claude()).unwrap();
    let (_, last) = events.until_end();
    assert!(
        matches!(
            last,
            SigninState::Failed {
                code: SigninCode::Cancelled,
                ..
            }
        ),
        "{last:?}"
    );
    eventually("no wait left", || fx.timer.live().is_empty());
    assert_eq!(fx.launches(), []);
}

#[test]
fn a_failed_install_keeps_the_sign_in_queued_until_one_succeeds() {
    let mut fx = Fixture::new("queued-failed", "ok-after:0");
    let install = fx.not_installed(&claude());
    let (agents, events) = fx.start_missing(claude());
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    agents.finish_install_for_tests(&claude(), None);
    assert!(matches!(
        events.install_state(),
        clauseo_agents::InstallState::Failed(_)
    ));
    settle();
    assert_eq!(agents.signin_state(&claude()), Some(SigninState::Queued));
    assert_eq!(fx.launches(), []);

    agents.finish_install_for_tests(&claude(), Some(install));
    let (seen, last) = events.until_end();
    assert_eq!(last, signed_in());
    assert_eq!(states(&seen), [waiting(), signed_in()]);
}

#[test]
fn a_new_sign_in_replaces_the_queued_one() {
    let mut fx = Fixture::new("queued-replace", "ok-after:0");
    let install = fx.not_installed(&claude());
    let (agents, events) = fx.start_missing(claude());
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    agents.sign_in(&claude(), Method::Console).unwrap();
    assert_eq!(events.state(), SigninState::Queued);
    agents.finish_install_for_tests(&claude(), Some(install));
    let (seen, last) = events.until_end();
    assert!(matches!(last, SigninState::SignedIn { .. }), "{last:?}");
    // The first job ended silently: one run, the second's.
    assert_eq!(states(&seen).len(), 2, "{seen:?}");
    let logins = fx.runs_of("login");
    assert_eq!(logins.len(), 1);
    assert!(logins[0].args.ends_with(&["--console".to_owned()]));
}

/// S18 security review, drift: a cancel that lands while the queued start's lookup runs ends the
/// job when the lookup returns: no `Waiting` and no run.
#[test]
fn a_cancel_during_the_lookup_starts_no_run() {
    use std::sync::{Arc, Condvar, Mutex};

    let mut fx = Fixture::new("queued-cancel-lookup", "never");
    // (entered, released), guarded together.
    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let held = Arc::clone(&gate);
    fx.config.install.resolver = Some(clauseo_agents::TestResolver(Arc::new(move |_: &str| {
        let (lock, changed) = &*held;
        let mut state = lock.lock().unwrap();
        state.0 = true;
        changed.notify_all();
        while !state.1 {
            state = changed.wait(state).unwrap();
        }
        Ok(vec![([127, 0, 0, 1], 0).into()])
    })));
    let (agents, events) = fx.start();
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    {
        let (lock, changed) = &*gate;
        let mut state = lock.lock().unwrap();
        while !state.0 {
            state = changed.wait(state).unwrap();
        }
    }
    agents.cancel_sign_in(&claude()).unwrap();
    {
        let (lock, changed) = &*gate;
        lock.lock().unwrap().1 = true;
        changed.notify_all();
    }
    let (seen, last) = events.until_end();
    assert_eq!(
        last,
        SigninState::Failed {
            code: SigninCode::Cancelled,
            at: now()
        }
    );
    assert!(!states(&seen).contains(&waiting()), "{seen:?}");
    settle();
    assert_eq!(fx.launches(), []);
}
