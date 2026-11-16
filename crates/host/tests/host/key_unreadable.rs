//! `agent.key_unreadable` (decision record 0048, R20 P5b A4): desktop refuses an agent's
//! launches with it once that agent's stored key was tried and its read failed or did not answer
//! in time. A launch meeting it asks desktop to read the key again (`HostConfig::with_key_retry`)
//! and waits at most 2 s on the host clock: lifted in time, it goes on with the key; still
//! refused, it fails with the code and no process starts. `agent.keys_loading` past its bound
//! (desktop never loaded the keys) refuses the same way.

use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_protocol::{AgentId, ErrorCode, StopReason, StreamId};

use crate::codex::{Recording, turn_failed};
use crate::keys_loading::{KEY, KEYS_WAIT, SlotKey, fake, until_keys_wait_armed};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, wait};

/// The host's bound on `agent.keys_loading` from its start.
const KEYS_LOAD_BOUND: Duration = Duration::from_secs(12);

#[test]
fn a_launch_meeting_key_unreadable_asks_for_a_read_again_and_goes_on_once_it_lifts() {
    let dir = Dir::new(
        "key_unreadable",
        &[(FAKE, Scenario::Local("launch-refused"))],
    );
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let slot = Arc::new(Mutex::new(None));
    let hook = SlotKey(Arc::clone(&slot));
    let retries: Arc<Mutex<Vec<AgentId>>> = Arc::default();
    let asked = Arc::clone(&retries);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_launch_refused(fake(), ErrorCode::AgentKeyUnreadable)
            .with_key_retry(Arc::new(move |agent: &AgentId| {
                asked.lock().unwrap().push(agent.clone());
            }))
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));

    // The read again does not succeed: the launch fails with the code, nothing launched.
    let (first, _) = host.prompt(thread, "Look at the matter.");
    until_keys_wait_armed(&host);
    assert_eq!(
        *retries.lock().unwrap(),
        vec![fake()],
        "desktop asked to read again"
    );
    host.clock.advance(KEYS_WAIT);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let failed = turn_failed(&events, first).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::AgentKeyUnreadable);
    assert!(failed.code.is_retryable());
    assert_eq!(launches.lock().unwrap().len(), 0, "no process started");

    // The lawyer tries again; this read succeeds: desktop lifts the refusal and the launch goes
    // on with the key.
    let (second, _) = host.prompt(thread, "Look again.");
    until_keys_wait_armed(&host);
    assert_eq!(
        retries.lock().unwrap().len(),
        2,
        "each refused launch asks once"
    );
    *slot.lock().unwrap() = Some(KEY.to_owned());
    wait(
        "set_launch_refused",
        host.handle.set_launch_refused(fake(), None),
    )
    .unwrap();
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    {
        let launches = launches.lock().unwrap();
        assert_eq!(launches.len(), 1);
        assert_eq!(
            launches[0].get(&OsString::from("ANTHROPIC_API_KEY")),
            Some(&OsString::from(KEY))
        );
    }
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn keys_loading_never_lifted_refuses_as_key_unreadable_past_the_bound() {
    let dir = Dir::new(
        "keys_loading_bound",
        &[(FAKE, Scenario::Local("launch-refused"))],
    );
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config.with_launch_refused(fake(), ErrorCode::AgentKeysLoading)
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    // Desktop's setup thread never loads the keys: past the bound, launches stop showing
    // "still starting" and refuse as an unreadable key, which a retry may get past.
    host.clock.advance(KEYS_LOAD_BOUND);
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    until_keys_wait_armed(&host);
    host.clock.advance(KEYS_WAIT);
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::AgentKeyUnreadable);
    assert_eq!(launches.lock().unwrap().len(), 0, "no process started");
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}
