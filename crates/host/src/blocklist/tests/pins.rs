//! The browser pins' part of each round (`updater/pins.rs`) against the fake server: a newer
//! manifest stored, the no-rollback rule, a refused or future-dated one, nothing published, a
//! stop, the round running while block list updates are off, the floor counting a stored copy
//! dated ahead of a clock set back, a heartbeat, and the same manifest again.

use super::{Fetching, Reply, Rig, Round, on};
use crate::blocklist::updater::PinsRound;
use crate::browser::pins::tests::{fixture, put};
use crate::browser::pins::{MANIFEST_FILE, PINS_FOLDER, SIGNATURE_FILE};
use crate::clock::Clock as _;

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
    // The same bytes again: taken, nothing to write.
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Unchanged);
    // With no copy at all, the first verified one is stored.
    let rig = Rig::new("pins-first");
    publish_pins(&rig, "pins");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Stored);
    assert_eq!(stored_pins(&rig), Some(fixture("pins")));
}

#[test]
fn an_older_or_lower_manifest_is_not_stored() {
    // Older, lower though later, and equally dated with other bytes.
    for name in ["pins-older", "pins-lower", "pins-no-extra"] {
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
    // The same manifest bytes under another signature (another run): taken, nothing written.
    let rig = Rig::new("pins-equal-bytes");
    put(&rig.dir.path().join("pins-bundled"), "pins");
    publish_pins(&rig, "pins-equal");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Unchanged);
    assert_eq!(stored_pins(&rig), None);
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

#[test]
fn with_the_clock_set_back_a_replayed_older_manifest_is_refused_and_the_stored_one_kept() {
    // The host clock (2026-11-07) is behind the stored 158's date (2026-12-20): that copy is
    // not used, but it is still the floor, so the replayed, genuinely signed 157 is refused.
    let rig = Rig::new("pins-clock-back");
    put(&rig.dir.path().join("pins-bundled"), "pins-older");
    put(&rig.data_dir().join(PINS_FOLDER), "pins-158-ahead");
    publish_pins(&rig, "pins");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::NotNewer);
    assert_eq!(stored_pins(&rig), Some(fixture("pins-158-ahead")));
}

#[test]
fn a_heartbeat_with_the_same_version_is_stored_and_resets_freshness() {
    let rig = Rig::new("pins-heartbeat");
    put(&rig.dir.path().join("pins-bundled"), "pins");
    publish_pins(&rig, "pins-heartbeat");
    let updater = rig.updater(rig.update());
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Stored);
    assert_eq!(stored_pins(&rig), Some(fixture("pins-heartbeat")));
    let source = rig.pins().source(&rig.data_dir());
    let used = source.newest(rig.clock.now().wall).unwrap();
    assert_eq!(used.version(), "157.0.8103.38");
    assert_eq!(used.published().as_str(), "2026-11-08T06:00:00Z");
    // Replaying the first one now: older, not the same bytes, refused; the heartbeat stays.
    publish_pins(&rig, "pins");
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::NotNewer);
    assert_eq!(stored_pins(&rig), Some(fixture("pins-heartbeat")));
}

/// Stage F's live manifest, through the real fetch path (the release URLs, keys and route),
/// against nothing stored: stored, verified with the compiled-in keys, and read. Not in CI (it
/// needs the network). By hand: `cargo nextest run -p clauseo-host --run-ignored only
/// the_live_manifest_is_fetched_verified_and_read`.
#[test]
#[ignore = "fetches stage F's live browser pins from github.com"]
fn the_live_manifest_is_fetched_verified_and_read() {
    use crate::browser::install::manifest::zip_url;
    use crate::browser::pins::{self, BrowserPins, PINS_PUBLIC_KEYS, Pins};
    use crate::clock::{Clock as _, SystemClock};

    let dir = crate::pipeline::tests::support::TempDir::new("pins-live");
    let data = dir.path().join("data");
    let seams = super::BlockListSeams::release(std::sync::Arc::default(), None);
    let live = super::Live::new(on());
    let updater = super::Updater::new(
        seams,
        &data,
        live,
        std::sync::Arc::new(SystemClock),
        BrowserPins::release(None),
    );
    assert_eq!(updater.refresh_pins(&|| false), PinsRound::Stored);
    let folder = data.join(PINS_FOLDER);
    let body = std::fs::read(folder.join(MANIFEST_FILE)).unwrap();
    let signature = std::fs::read(folder.join(SIGNATURE_FILE)).unwrap();
    let pins = Pins::verify(&body, &signature, PINS_PUBLIC_KEYS).unwrap();
    let chrome = pins.pin("chrome", "win64").unwrap();
    let now = SystemClock.now().wall;
    println!("published {}", pins.published().as_str());
    println!("version {}", pins.version());
    println!("run {}", pins.run());
    println!("chrome win64 sha256 {} size {}", chrome.sha256, chrome.size);
    println!("url {}", zip_url(pins.version(), "win64", "chrome"));
    println!(
        "expired {} notice {:?}",
        pins::expired(pins.published(), now),
        pins::notice(pins.published(), now)
    );
    assert!(pins::accept(&pins, None, None));
    assert!(!pins.is_ahead_of(now));
    assert!(!pins::expired(pins.published(), now));
    assert_eq!(
        zip_url(pins.version(), "win64", "chrome"),
        format!(
            "https://storage.googleapis.com/chrome-for-testing-public/{}/win64/chrome-win64.zip",
            pins.version()
        )
    );
    // The stored copy is now the floor and the copy in use.
    let source = BrowserPins::release(None).source(&data);
    assert_eq!(
        source.floor().map(|floor| floor.run().to_owned()),
        Some(pins.run().to_owned())
    );
    assert_eq!(
        source.newest(now).map(|used| used.run().to_owned()),
        Some(pins.run().to_owned())
    );
}
