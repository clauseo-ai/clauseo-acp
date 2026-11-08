//! Keys and followers, after P5a's review: a failed keychain read refuses `main`'s first key,
//! a `main` and a `settings` key at once leave one consistent outcome, and a new follower
//! drops the closed ones.

use std::sync::Arc;

use clauseo_protocol::{CommandOutcome, ErrorCode, SetupAction};

use super::{MAIN, SECRET, SETTINGS, claude, command, stubbed};
use crate::setup::{KeyWrite, SetupEvents};
use crate::test_support::block_on;

#[test]
fn a_failed_keychain_read_refuses_main_s_first_key_and_stores_nothing() {
    let (setup, stub) = stubbed(true);
    stub.script(
        SetupAction::SetAgentKey,
        Ok(CommandOutcome::Done),
        Vec::new(),
    );
    stub.fail_reads(true);
    let refused = block_on(setup.run(MAIN, command(SetupAction::SetAgentKey))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::Internal);
    assert!(!refused.message().contains(SECRET));
    assert!(!stub.has_key(&claude()), "a failed read is not \"absent\"");
    assert_eq!(stub.stores(), []);
    stub.fail_reads(false);
    assert_eq!(
        block_on(setup.run(MAIN, command(SetupAction::SetAgentKey))),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(stub.stores(), [(claude(), KeyWrite::OnlyIfAbsent)]);
}

#[test]
fn main_and_settings_keys_at_once_leave_one_consistent_outcome() {
    for round in 0..50 {
        let (setup, stub) = stubbed(true);
        stub.script(
            SetupAction::SetAgentKey,
            Ok(CommandOutcome::Done),
            Vec::new(),
        );
        let setup = Arc::new(setup);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let send = |label: &'static str| {
            let setup = Arc::clone(&setup);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                block_on(setup.run(label, command(SetupAction::SetAgentKey)))
            })
        };
        let main = send(MAIN);
        let settings = send(SETTINGS);
        let (main, settings) = (main.join().unwrap(), settings.join().unwrap());
        assert_eq!(
            settings,
            Ok(CommandOutcome::Done),
            "round {round}: settings may replace"
        );
        let stores = stub.stores();
        match main {
            // main stored first, then settings replaced it.
            Ok(CommandOutcome::Done) => assert_eq!(
                stores,
                [
                    (claude(), KeyWrite::OnlyIfAbsent),
                    (claude(), KeyWrite::Replace)
                ],
                "round {round}"
            ),
            // settings stored first: main may not replace it.
            Err(error) => {
                assert_eq!(error.code(), ErrorCode::SettingsKeyStored, "round {round}");
                assert_eq!(stores, [(claude(), KeyWrite::Replace)], "round {round}");
            }
            Ok(other) => panic!("round {round}: {other:?}"),
        }
        assert!(stub.has_key(&claude()));
    }
}

#[test]
fn a_new_follower_drops_the_closed_ones() {
    let events = SetupEvents::default();
    let first = events.follow();
    let second = events.follow();
    assert_eq!(events.followers(), 2);
    drop(first);
    drop(second);
    let _third = events.follow();
    assert_eq!(
        events.followers(),
        1,
        "closed followers go when a new one follows"
    );
}
