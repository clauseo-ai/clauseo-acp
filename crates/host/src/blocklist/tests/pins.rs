//! The browser pins' part of each round (`updater/pins.rs`) against the fake server: a newer
//! manifest stored, the no-rollback rule, a refused or future-dated one, nothing published, a
//! stop, and the round running while block list updates are off.

use super::{Fetching, Reply, Rig, Round, on};
use crate::blocklist::updater::PinsRound;
use crate::browser::pins::tests::{fixture, put};
use crate::browser::pins::{MANIFEST_FILE, PINS_FOLDER, SIGNATURE_FILE};

/// `rig`'s pins server publishes the fixture `name`.
fn publish_pins(rig: &Rig, name: &str) {
    let (body, signature) = fixture(name);
    rig.pins_server.set("/pins.json", Reply::Body(body));
    rig.pins_server
        .set("/pins.json.minisig", Reply::Body(signature));
}

/// `rig`'s stored manifest and signature, if any.
fn stored_pins(rig: &Rig) -> Option<(Vec<u8>, Vec<u8>)> {
    let folder = rig.data_dir().join(PINS_FOLDER);
    let body = std::fs::read(folder.join(MANIFEST_FILE)).ok()?;
    let signature = std::fs::read(folder.join(SIGNATURE_FILE)).ok()?;
    Some((body, signature))
}

#[test]
fn a_newer_manifest_is_stored_and_the_same_one_again_is_not() {
    let rig = Rig::new("pins-newer");
    put(&rig.dir.path().join("pins-bundled"), "pins");
    publish_pins(&rig, "pins-newer");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Stored);
    assert_eq!(stored_pins(&rig), Some(fixture("pins-newer")));
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::NotNewer);
    // With no copy at all, the first verified one is stored.
    let rig = Rig::new("pins-first");
    publish_pins(&rig, "pins");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Stored);
    assert_eq!(stored_pins(&rig), Some(fixture("pins")));
}

#[test]
fn an_older_or_lower_manifest_is_not_stored() {
    for name in ["pins-older", "pins-lower", "pins-equal"] {
        let rig = Rig::new(&format!("{name}-rollback"));
        put(&rig.dir.path().join("pins-bundled"), "pins");
        publish_pins(&rig, name);
        let updater = rig.updater(rig.update());
        assert_eq!(
            updater.refresh_pins(&|| false),
            PinsRound::NotNewer,
            "{name}"
        );
        assert_eq!(stored_pins(&rig), None, "{name}");
    }
}

#[test]
fn a_refused_or_future_dated_manifest_is_not_stored() {
    for name in ["pins-by-c", "pins-schema-2", "pins-ahead"] {
        let rig = Rig::new(&format!("{name}-refused"));
        publish_pins(&rig, name);
        let updater = rig.updater(rig.update());
        assert_eq!(
            updater.refresh_pins(&|| false),
            PinsRound::Refused,
            "{name}"
        );
        assert_eq!(stored_pins(&rig), None, "{name}");
    }
    // A body beside another's signature.
    let rig = Rig::new("pins-torn");
    let (body, _) = fixture("pins-newer");
    let (_, signature) = fixture("pins");
    rig.pins_server.set("/pins.json", Reply::Body(body));
    rig.pins_server
        .set("/pins.json.minisig", Reply::Body(signature));
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Refused);
}

#[test]
fn nothing_published_and_a_stop_store_nothing() {
    let rig = Rig::new("pins-404");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::NotPublished);
    rig.pins_server
        .set("/pins.json.minisig", Reply::Status(500));
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Failed);
    publish_pins(&rig, "pins");
    assert_eq!(updater.refresh_pins(&|| true), PinsRound::Stopped);
    assert_eq!(stored_pins(&rig), None);
}

#[test]
fn the_pins_are_fetched_while_block_list_updates_are_off() {
    let rig = Rig::new("pins-updates-off");
    super::put(&rig.bundled(), "list");
    rig.publish(super::pair("list-newer"));
    publish_pins(&rig, "pins");
    let updater = rig.started(rig.update());
    rig.live.set(Fetching {
        updates: false,
        ..on()
    });
    assert_eq!(updater.run_once(&|| false), Round::Off);
    assert_eq!(rig.server.hits(), Vec::<String>::new());
    assert_eq!(stored_pins(&rig), Some(fixture("pins")));
}
