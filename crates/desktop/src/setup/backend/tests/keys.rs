//! Keys through the real backend and the real `KeyCache` over a memory keychain: a failed read
//! refuses `main`'s first key, `main` racing `settings` and a set racing a clear leave memory
//! as the keychain and the windows told the same, and a key a failed read left out at start is
//! taken in when `main`'s is refused.

use std::sync::{Arc, Barrier};

use clauseo_agents::SigninState;
use clauseo_agents::keys::Keychain;
use clauseo_protocol::{
    AgentId, CommandOutcome, ErrorCode, HostError, SetupEvent, SignIn, SignInMethod,
};
use serde_json::json;

use super::fakes::{HostCall, Rig, command, rig, rig_on, setup_events};
use super::{MAIN, SETTINGS, claude};
use crate::test_support::block_on;

const MAIN_KEY: &str = "sk-ant-api03-MAIN-KEY-0001";
const SETTINGS_KEY: &str = "sk-ant-api03-SETTINGS-KEY-0002";

fn set_key(key: &str) -> clauseo_protocol::SetupCommand {
    command("set_agent_key", &claude(), &json!({ "key": key }))
}

/// The keychain's key for Claude, read with reads working.
fn stored(rig: &Rig) -> Option<String> {
    rig.keychain
        .load(&claude())
        .unwrap()
        .map(|key| key.as_str().to_owned())
}

/// Memory's key for Claude.
fn cached(rig: &Rig) -> Option<String> {
    rig.backend
        .keys
        .get(&claude())
        .map(|key| key.as_str().to_owned())
}

/// The latest `SignInChanged` for `agent`, as a new follower gets it.
fn shown(rig: &Rig, agent: &AgentId) -> Option<SignIn> {
    let mut feed = rig.events.follow();
    setup_events(&mut feed)
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent: of, state } if of == *agent => Some(state),
            _ => None,
        })
        .next_back()
}

fn shows_api_key(rig: &Rig) -> bool {
    matches!(
        shown(rig, &claude()),
        Some(SignIn::SignedIn {
            method: SignInMethod::ApiKey,
            ..
        })
    )
}

/// Runs `first` and `second` from their windows at once, after a barrier.
fn at_once(
    rig: &Rig,
    first: (&'static str, clauseo_protocol::SetupCommand),
    second: (&'static str, clauseo_protocol::SetupCommand),
) -> [Result<CommandOutcome, HostError>; 2] {
    let setup = Arc::new(rig.setup());
    let barrier = Arc::new(Barrier::new(2));
    let send = |(label, sent): (&'static str, clauseo_protocol::SetupCommand)| {
        let setup = Arc::clone(&setup);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            block_on(setup.run(label, sent))
        })
    };
    let (first, second) = (send(first), send(second));
    [first.join().unwrap(), second.join().unwrap()]
}

#[test]
fn a_failed_read_refuses_main_s_first_key_and_stores_nothing_on_the_real_backend() {
    let rig = rig(true);
    rig.keychain.fail_reads(true);
    let refused = block_on(rig.setup().run(MAIN, set_key(MAIN_KEY))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::Internal);
    assert!(!refused.message().contains(MAIN_KEY));
    rig.keychain.fail_reads(false);
    assert_eq!(stored(&rig), None, "a failed read is not \"absent\"");
    assert_eq!(cached(&rig), None);
    assert_eq!(shown(&rig, &claude()), None, "nothing told");
    assert_eq!(
        block_on(rig.setup().run(MAIN, set_key(MAIN_KEY))),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(stored(&rig).as_deref(), Some(MAIN_KEY));
    assert_eq!(cached(&rig).as_deref(), Some(MAIN_KEY));
}

#[test]
fn main_racing_settings_leaves_one_outcome_in_the_keychain_and_in_memory() {
    for round in 0..50 {
        let rig = rig(true);
        let [main, settings] = at_once(
            &rig,
            (MAIN, set_key(MAIN_KEY)),
            (SETTINGS, set_key(SETTINGS_KEY)),
        );
        assert_eq!(settings, Ok(CommandOutcome::Done), "round {round}");
        match main {
            // main stored first, then settings replaced it; or settings first, refusing main.
            Ok(CommandOutcome::Done) => {}
            Err(error) => assert_eq!(error.code(), ErrorCode::SettingsKeyStored, "round {round}"),
            Ok(other) => panic!("round {round}: {other:?}"),
        }
        assert_eq!(stored(&rig).as_deref(), Some(SETTINGS_KEY), "round {round}");
        assert_eq!(cached(&rig), stored(&rig), "round {round}");
        assert!(shows_api_key(&rig), "round {round}");
    }
}

#[test]
fn a_set_racing_a_clear_leaves_memory_as_the_keychain_and_the_window_agreeing() {
    for round in 0..100 {
        let rig = rig(true);
        rig.backend
            .compose
            .signin(&claude(), SigninState::SignedOut { found: None });
        assert_eq!(
            block_on(rig.setup().run(SETTINGS, set_key(MAIN_KEY))),
            Ok(CommandOutcome::Done)
        );
        let clear = command("clear_agent_key", &claude(), &json!({}));
        let answers = at_once(&rig, (SETTINGS, set_key(SETTINGS_KEY)), (SETTINGS, clear));
        assert_eq!(
            answers,
            [Ok(CommandOutcome::Done), Ok(CommandOutcome::Done)]
        );
        let stored = stored(&rig);
        assert!(
            stored.is_none() || stored.as_deref() == Some(SETTINGS_KEY),
            "round {round}"
        );
        assert_eq!(cached(&rig), stored, "round {round}");
        assert_eq!(
            rig.backend.keys.has(&claude()),
            stored.is_some(),
            "round {round}"
        );
        assert_eq!(shows_api_key(&rig), stored.is_some(), "round {round}");
    }
}

#[test]
fn a_key_a_failed_read_left_out_at_start_is_taken_in_when_main_s_is_refused() {
    let keychain = Keychain::memory();
    keychain.store(&claude(), SETTINGS_KEY).unwrap();
    keychain.fail_reads(true);
    let rig = rig_on(true, keychain);
    rig.keychain.fail_reads(false);
    assert_eq!(cached(&rig), None, "the read at start failed");
    let refused = block_on(rig.setup().run(MAIN, set_key(MAIN_KEY))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::SettingsKeyStored);
    assert_eq!(
        stored(&rig).as_deref(),
        Some(SETTINGS_KEY),
        "main replaced nothing"
    );
    assert_eq!(
        cached(&rig).as_deref(),
        Some(SETTINGS_KEY),
        "memory took it in"
    );
    assert!(shows_api_key(&rig));
}

#[test]
fn with_no_keychain_a_key_on_a_reused_login_is_refused_before_any_leave() {
    let rig = rig_on(true, Keychain::none());
    super::leave::reuse(&rig, &claude());
    let setup = rig.setup();
    let answer = rig.pumped(setup.run(SETTINGS, set_key(SETTINGS_KEY)));
    let error = answer.unwrap_err();
    assert_eq!(error.code(), ErrorCode::Internal);
    assert!(error.message().contains("no keychain"), "{error:?}");
    assert_eq!(
        rig.host.calls(),
        Vec::<HostCall>::new(),
        "no leave, no call"
    );
    assert!(rig.calls().is_empty(), "S18 was asked nothing");
    assert!(rig.backend.wiring.reused.get(&claude()));
}
