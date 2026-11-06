//! R24 PR5's no-key check made real (the P1 review): with the log file on and the connectors
//! wired, a distinctive API key is added (`AddApiKey` runs, and the key is stored in the
//! stand-in keychain), a malformed one is refused, the balance is read and an agent runs a
//! turn; then no file holds either key: not the log, not the policy file, not anywhere under the
//! data folder (the agent homes, `<data>/agents`, included when an agent kind makes one) or the
//! agents' home folder, nor the connectors' state folder.
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::{DomainEvent, ErrorCode, StreamId};
use clauseo_settings::Locations;

use crate::connector_rig::{Rig, Start, add_key, refresh, signed_in};
use crate::connectors::files_holding;
use crate::observe::Events;
use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

const AGENT: &str = "fake-writer";

/// Distinctive, and not a key shape the connector accepts.
const BAD_KEY: &str = "cdk_live_ZZnotakeyZZ_7f3a9c";

fn turn_over(events: &Events) -> bool {
    events.events().any(|event| {
        matches!(
            event,
            DomainEvent::TurnEnded(_) | DomainEvent::TurnFailed(_)
        )
    })
}

#[test]
fn an_added_key_is_in_no_file_of_the_data_folder_the_log_or_the_homes() {
    let dir = Dir::new(
        "connector_no_key",
        &[(AGENT, Scenario::Bundled("files-one-turn"))],
    );
    dir.set_policy(r#"{"tools": {"browser": false}}"#);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let guard = log_file::install(&Locations::under_root(&logs));
    assert!(guard.is_active(), "the log did not start");

    let mut rig = Rig::start(&dir, Start::default());
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    rig.ok(add_key(&key));
    rig.state_until("signed in with the key", |state| signed_in(state).is_some());
    let refused = rig.dispatch(add_key(BAD_KEY)).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    rig.ok(refresh());
    let (_matter, thread, _folder) = matter_with_thread(
        &dir,
        &rig.host,
        "Keyless matter",
        &[("a.txt", "a\n")],
        AGENT,
    );
    let mut events = rig.host.events(StreamId::Thread(thread));
    rig.host.prompt(thread, "Tidy the notes");
    events.until("the turn ends", turn_over);
    // The connectors' folder goes with the rig: searched now, with its state file and day count.
    for needle in [key.as_str(), BAD_KEY] {
        let holding = files_holding(rig.scratch(), needle.as_bytes());
        assert!(holding.is_empty(), "a key is in {holding:?}");
    }
    rig.finish();
    drop(guard);

    let log = files_holding(&logs, b"host");
    assert!(!log.is_empty(), "the log holds nothing");
    for needle in [key.as_str(), BAD_KEY] {
        for folder in [dir.data(), dir.home()] {
            let holding = files_holding(folder, needle.as_bytes());
            assert!(holding.is_empty(), "a key is in {holding:?}");
        }
        let policy = std::fs::read_to_string(dir.data().join("..").join("policy.json")).unwrap();
        assert!(!policy.contains(needle));
    }
}
