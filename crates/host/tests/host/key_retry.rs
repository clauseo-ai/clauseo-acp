//! The API-key `authenticate` retry (R20 brief, "API keys"; decision record 0048, P4b): a Codex
//! agent launched with the profile hook's `CODEX_API_KEY` whose login check answered `Own`
//! gets one `authenticate` with `api-key` when it answers auth-required, then the call goes
//! again. A second auth-required holds the turn for the sign-in; a shared login, or no check,
//! never authenticates. Each scenario expects at most one `authenticate`: any other is a
//! mismatch. The host's log line before each `authenticate` shows whether it tried (a
//! `session/new` that fails leaves no detach to carry a mismatch). The key never reaches a log
//! or an event.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{HostConfig, LoginLink, ProfileHook};
use clauseo_protocol::{
    AgentId, CommandOutcome, DomainEvent, ErrorCode, HostCommand, StopReason, StreamId, TurnId,
};
use serde_json::{Map, Value};

use crate::codex::{CODEX, codex_dir, turn_failed};
use crate::login_check::{Fake, Keep, codex};
use crate::observe::Events;
use crate::queue::held;
use crate::support::{Dir, Running, matter_with_thread, wait};

/// The key the hook launches Codex with; no log line or event may hold it.
const KEY: &str = "sk-test-key-retry-0123456789";

/// A profile hook that launches every agent with [`KEY`] in `CODEX_API_KEY`.
struct WithKey;

impl ProfileHook for WithKey {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::from([(OsString::from("CODEX_API_KEY"), OsString::from(KEY))])
    }
}

/// The log line the host writes before its `authenticate`.
const TRIED: &str = "authenticate api-key";

/// Keeps this test process's log lines (nextest runs each test in its own process, so the
/// default subscriber is this test's alone).
fn keep_logs() -> Arc<Mutex<Vec<String>>> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let _set = tracing::subscriber::set_global_default(Keep(Arc::clone(&lines)));
    lines
}

/// Asserts the host never tried `authenticate`.
fn assert_never_tried(lines: &Mutex<Vec<String>>) {
    let lines = lines.lock().unwrap();
    assert!(lines.iter().all(|line| !line.contains(TRIED)), "{lines:#?}");
}

/// Starts `dir`'s host with the key hook and, when given, a login check answering `login`.
fn start(dir: &Dir, login: Option<LoginLink>) -> Running {
    dir.start_with(0, |config: HostConfig| {
        let config = config.with_profile_hook(Box::new(WithKey));
        match login {
            Some(link) => config.with_login_check(Fake::new(link)),
            None => config,
        }
    })
}

/// Sends one prompt on a new Codex thread and waits for its turn to end.
fn one_prompt(dir: &Dir, host: &Running) -> (Events, TurnId, bool) {
    let (matter, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    let held = held(host, matter, thread);
    (events, turn, held)
}

/// The turn failed for the sign-in, and the agent saw no `authenticate` it did not expect.
fn assert_held_for_sign_in(events: &Events, turn: TurnId, held: bool) {
    events.assert_no_mismatch();
    let failed = turn_failed(events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(
        failed.code,
        ErrorCode::AgentAuthRequired,
        "{}",
        events.dump()
    );
    assert!(held, "{}", events.dump());
}

#[test]
fn a_key_and_an_own_login_authenticate_once_and_the_turn_goes_on() {
    let lines = keep_logs();
    let dir = codex_dir("key_retry_ok", "codex-auth-required-key");
    let host = start(&dir, Some(LoginLink::Own));
    let (events, turn, held) = one_prompt(&dir, &host);
    // The scenario took exactly one `authenticate` with `api-key`, then `session/new` again.
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).map(|ended| ended.stop),
        Some(StopReason::EndTurn),
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(turn), "Signed in with the key.");
    assert!(!held);
    assert!(!events.dump().contains(KEY));
    host.shutdown();

    let lines = lines.lock().unwrap();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("authenticate api-key")),
        "{lines:#?}"
    );
    assert!(lines.iter().all(|line| !line.contains(KEY)), "{lines:#?}");
}

#[test]
fn a_second_auth_required_after_authenticate_holds_the_turn() {
    let dir = codex_dir("key_retry_again", "codex-auth-required-key-again");
    let host = start(&dir, Some(LoginLink::Own));
    let (mut events, turn, held) = one_prompt(&dir, &host);
    assert_held_for_sign_in(&events, turn, held);
    let head = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.is_at_head());
    events.until("the turn put back", |events| {
        events.position(head).is_some()
    });
    assert!(!events.dump().contains(KEY));
    host.shutdown();
}

#[test]
fn a_shared_login_never_authenticates() {
    let lines = keep_logs();
    let dir = codex_dir("key_retry_shared", "codex-auth-required");
    let host = start(&dir, Some(LoginLink::Shared));
    let (events, turn, held) = one_prompt(&dir, &host);
    assert_held_for_sign_in(&events, turn, held);
    host.shutdown();

    // Reused (a link exists) with the file shared: the launch goes on, and still no retry.
    let dir = codex_dir("key_retry_reused", "codex-auth-required");
    let host = start(&dir, Some(LoginLink::Shared));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    let (events, turn, held) = one_prompt(&dir, &host);
    assert_held_for_sign_in(&events, turn, held);
    host.shutdown();
    assert_never_tried(&lines);
}

#[test]
fn without_a_login_check_a_key_never_authenticates() {
    let lines = keep_logs();
    let dir = codex_dir("key_retry_unchecked", "codex-auth-required");
    let host = start(&dir, None);
    let (events, turn, held) = one_prompt(&dir, &host);
    assert_held_for_sign_in(&events, turn, held);
    host.shutdown();
    assert_never_tried(&lines);
}

/// The host's log line for the chunk the agent streams while it holds `authenticate`: no
/// prompt is in flight, so the text is dropped.
const HELD: &str = "agent text outside a turn was dropped";

/// Waits (at most the watchdog's 20 s) for a log line containing `what`.
fn until_logged(lines: &Mutex<Vec<String>>, what: &str) {
    let logged = || lines.lock().unwrap().iter().any(|line| line.contains(what));
    wait(what, async {
        while !logged() {
            async_io::Timer::after(Duration::from_millis(10)).await;
        }
    });
}

#[test]
fn a_stop_during_authenticate_sends_no_second_prompt_and_the_turn_ends_cancelled() {
    let lines = keep_logs();
    let dir = codex_dir("key_retry_stop", "codex-auth-required-key-stop");
    let host = start(&dir, Some(LoginLink::Own));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    // The agent holds `authenticate` and streams a chunk, which the host drops: the first
    // prompt already answered, so no prompt is in flight when the Stop comes.
    until_logged(&lines, HELD);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    // A second `session/prompt` is a mismatch: the scenario ends after `authenticate`.
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    assert!(turn_failed(&events, turn).is_none(), "{}", events.dump());
    host.shutdown();
}
