//! `agent.keys_loading` and the refusals seeded at the host's start (decision record 0048,
//! ruling 4 and the security review's M3): `HostConfig::with_launch_refused` refuses an id's
//! launches before any can come. A launch meeting `agent.keys_loading` waits at most 2 s on
//! the host clock: when the refusal is still there it fails with that code and nothing
//! launches; when desktop lifts it in time (the keys loaded), the launch goes on with the
//! profile hook's key. A seeded `signin.policy_unreadable` refuses at once.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::ProfileHook;
use clauseo_protocol::{AgentId, ErrorCode, StopReason, StreamId};
use serde_json::{Map, Value};

use crate::codex::{Recording, turn_failed};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The key the hook hands out once the "keys" are loaded.
pub(crate) const KEY: &str = "sk-test-keys-loading-0123456789";

/// The host's wait for an `agent.keys_loading` refusal.
pub(crate) const KEYS_WAIT: Duration = Duration::from_secs(2);

/// The fake agent's id.
pub(crate) fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

/// A profile hook adding `ANTHROPIC_API_KEY` once its slot holds a key, as desktop's does once
/// the keychain has loaded.
pub(crate) struct SlotKey(pub(crate) Arc<Mutex<Option<String>>>);

impl ProfileHook for SlotKey {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|key| (OsString::from("ANTHROPIC_API_KEY"), OsString::from(key)))
            .collect()
    }
}

/// Waits until the host has armed a timer of [`KEYS_WAIT`] (a launch began its wait).
pub(crate) fn until_keys_wait_armed(host: &Running) {
    for _ in 0..256 {
        if wait("a timer armed", host.clock.next_armed()) == KEYS_WAIT {
            return;
        }
    }
    panic!("no launch waited for the keys");
}

#[test]
fn a_launch_while_the_keys_load_waits_2_s_then_fails_and_one_woken_in_time_gets_the_key() {
    let dir = Dir::new("keys_loading", &[(FAKE, Scenario::Local("launch-refused"))]);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let slot = Arc::new(Mutex::new(None));
    let hook = SlotKey(Arc::clone(&slot));
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_launch_refused(fake(), ErrorCode::AgentKeysLoading)
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));

    // The keys never load: the launch waits, then fails with the code, and nothing launched.
    let (first, _) = host.prompt(thread, "Look at the matter.");
    until_keys_wait_armed(&host);
    let almost = KEYS_WAIT.saturating_sub(Duration::from_millis(1));
    host.clock.advance(almost);
    host.settle();
    events.drain_ready();
    assert!(turn_failed(&events, first).is_none(), "{}", events.dump());
    host.clock.advance(Duration::from_millis(1));
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let failed = turn_failed(&events, first).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::AgentKeysLoading);
    assert!(failed.code.is_retryable());
    assert_eq!(launches.lock().unwrap().len(), 0, "no process started");

    // The keys load while a launch waits: desktop lifts the refusal, the launch goes on at
    // once, with the key.
    let (second, _) = host.prompt(thread, "Look again.");
    until_keys_wait_armed(&host);
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

    // After the load, launches start without waiting; the running agent takes the next.
    let (third, _) = host.prompt(thread, "And once more.");
    assert_eq!(events.until_turn_ended(third).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_policy_refusal_seeded_at_the_start_refuses_at_once_and_keys_loading_is_taken() {
    let dir = Dir::new(
        "keys_loading_policy",
        &[(FAKE, Scenario::Local("launch-refused"))],
    );
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config.with_launch_refused(fake(), ErrorCode::SigninPolicyUnreadable)
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::Failed);
    let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
    assert_eq!(failed.code, ErrorCode::SigninPolicyUnreadable);
    assert_eq!(launches.lock().unwrap().len(), 0, "nothing launched");
    // `set_launch_refused` takes `agent.keys_loading` too.
    wait(
        "set_launch_refused",
        host.handle
            .set_launch_refused(fake(), Some(ErrorCode::AgentKeysLoading)),
    )
    .unwrap();
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}
