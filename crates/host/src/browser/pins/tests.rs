//! The browser pins: the signature by either key, the trusted comment against the body, the
//! body's checks, the no-rollback rule, the newer of two copies, and the freshness notice.
//! Fixtures: `tests/data/browser-pins/fixtures.json` (`make_fixtures.py` writes it, signed by
//! the block list's throwaway test keys: A plays the current key, B the next, C neither).

use std::path::Path;

use clauseo_blocklist::{Error, Generated, key_line};
use clauseo_protocol::{Timestamp, ToolNotice};

use super::{
    MANIFEST_FILE, PINS_KEY_CURRENT, PINS_KEY_NEXT, PINS_PUBLIC_KEYS, Pin, Pins, PinsError,
    PinsSource, SIGNATURE_FILE, accept, expired, notice,
};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The fixtures file.
const FIXTURES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/browser-pins/fixtures.json"
));

/// The fixture `name`: its body and its signature.
pub(crate) fn fixture(name: &str) -> (Vec<u8>, Vec<u8>) {
    let all: serde_json::Value = serde_json::from_str(FIXTURES).unwrap();
    let one = &all[name];
    let text = |field: &str| one[field].as_str().unwrap().as_bytes().to_vec();
    (text("body"), text("minisig"))
}

/// The key line of the block list's test key `letter` (`a`, `b` or `c`).
pub(crate) fn test_key(letter: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../blocklist/tests/data")
        .join(format!("key-{letter}.pub"));
    let text = std::fs::read_to_string(path).unwrap();
    text.lines().nth(1).unwrap().to_owned()
}

/// Writes the fixture `name` into `folder` under the published names.
pub(crate) fn put(folder: &Path, name: &str) {
    write::create_dir_all(folder).unwrap();
    let (body, signature) = fixture(name);
    write::write_file(&folder.join(MANIFEST_FILE), &body).unwrap();
    write::write_file(&folder.join(SIGNATURE_FILE), &signature).unwrap();
}

/// The wall time at `text` (RFC 3339 UTC).
pub(crate) fn at(text: &str) -> Timestamp {
    Timestamp::new(Generated::parse(text).unwrap().unix_seconds() * 1000).unwrap()
}

/// The fixture `name` checked against test keys A and B.
fn check(name: &str) -> Result<Pins, PinsError> {
    let (body, signature) = fixture(name);
    Pins::verify(&body, &signature, &[&test_key("a"), &test_key("b")])
}

#[test]
fn a_manifest_signed_by_the_current_test_key_verifies_and_reads() {
    let (body, signature) = fixture("pins");
    let pins = Pins::verify(&body, &signature, &[&test_key("a")]).unwrap();
    assert_eq!(pins.published().as_str(), "2026-11-07T06:00:00Z");
    assert_eq!(pins.version(), "157.0.8103.38");
    assert_eq!(pins.run(), "1234");
    assert_eq!(
        pins.pin("chrome", "win64"),
        Some(&Pin {
            sha256: "ae0deef27d449e1d7529df7705d609bab102de48464da13567ed3dd2ee1a0586".to_owned(),
            size: 190_000_000,
        })
    );
    assert_eq!(
        pins.pin("chrome-headless-shell", "win64")
            .map(|pin| pin.size),
        Some(126_513_060)
    );
    assert_eq!(pins.pin("chrome", "win-arm64"), None);
    assert_eq!(pins.pin("chromedriver", "win64"), None);
}

#[test]
fn a_manifest_signed_by_the_next_test_key_verifies() {
    let (body, signature) = fixture("pins-by-b");
    let pins = Pins::verify(&body, &signature, &[&test_key("a"), &test_key("b")]).unwrap();
    assert_eq!(pins.version(), "157.0.8103.38");
    // Only the next key in the set: still verifies by it alone.
    Pins::verify(&body, &signature, &[&test_key("b")]).unwrap();
}

#[test]
fn a_manifest_signed_by_an_unknown_key_is_refused() {
    assert_eq!(
        check("pins-by-c"),
        Err(PinsError::Signature(Error::Signature))
    );
    let (body, signature) = fixture("pins");
    assert_eq!(
        Pins::verify(&body, &signature, &[&test_key("b")]),
        Err(PinsError::Signature(Error::Signature))
    );
}

#[test]
fn a_changed_body_or_a_body_beside_another_signature_is_refused() {
    let (body, signature) = fixture("pins");
    let changed = String::from_utf8(body)
        .unwrap()
        .replace("190000000", "190000001");
    assert_eq!(
        Pins::verify(changed.as_bytes(), &signature, &[&test_key("a")]),
        Err(PinsError::Signature(Error::Signature))
    );
    let (newer, _) = fixture("pins-newer");
    assert_eq!(
        Pins::verify(&newer, &signature, &[&test_key("a")]),
        Err(PinsError::Signature(Error::Signature))
    );
}

#[test]
fn the_compiled_in_keys_are_the_two_minisign_keys_and_verify_no_test_manifest() {
    assert_eq!(PINS_PUBLIC_KEYS, &[PINS_KEY_CURRENT, PINS_KEY_NEXT]);
    assert_eq!(
        key_line(PINS_KEY_CURRENT),
        Some("RWSzpZVjE2pJxXIEKdv4mCU35RlDGmGauNo8xYF4ZGw444xd0UPkSd7M")
    );
    assert_eq!(
        key_line(PINS_KEY_NEXT),
        Some("RWQX8RqVRHxVuCarDaYD7ELbrODzRuomq6MPX9A86EtrMiPrI4XyGdys")
    );
    for file in PINS_PUBLIC_KEYS {
        assert!(file.starts_with("untrusted comment: minisign public key "));
        let line = key_line(file).unwrap();
        // Each parses as a minisign public key: alone in the set, it gives "does not verify"
        // for a test manifest, not "no key".
        let (body, signature) = fixture("pins");
        assert_eq!(
            clauseo_blocklist::trusted_comment(&body, &signature, &[file]),
            Err(Error::Signature)
        );
        assert!(![test_key("a"), test_key("b"), test_key("c")].contains(&line.to_owned()));
    }
    // Neither is a block list key.
    assert!(
        clauseo_blocklist::PUBLIC_KEYS
            .iter()
            .all(|key| !PINS_PUBLIC_KEYS.contains(key))
    );
    for name in ["pins", "pins-by-b", "pins-by-c"] {
        let (body, signature) = fixture(name);
        assert_eq!(
            Pins::verify(&body, &signature, PINS_PUBLIC_KEYS),
            Err(PinsError::Signature(Error::Signature)),
            "{name}"
        );
    }
}

#[test]
fn a_trusted_comment_that_differs_from_the_body_or_the_form_is_refused() {
    for (name, error) in [
        ("pins-comment-published", PinsError::Mismatch),
        ("pins-comment-version", PinsError::Mismatch),
        ("pins-comment-no-run", PinsError::Comment),
        ("pins-comment-run-letters", PinsError::Comment),
        ("pins-comment-run-empty", PinsError::Comment),
        ("pins-comment-run-long", PinsError::Comment),
        ("pins-comment-other", PinsError::Comment),
    ] {
        assert_eq!(check(name), Err(error), "{name}");
    }
}

#[test]
fn a_tampered_trusted_comment_is_refused_by_the_signature() {
    let (body, signature) = fixture("pins");
    let text = String::from_utf8(signature).unwrap();
    let tampered = text.replace("run=1234", "run=1235");
    assert_ne!(tampered, text);
    assert_eq!(
        Pins::verify(&body, tampered.as_bytes(), &[&test_key("a")]),
        Err(PinsError::Signature(Error::Signature))
    );
}

#[test]
fn a_schema_other_than_1_is_refused() {
    assert_eq!(check("pins-schema-2"), Err(PinsError::Schema));
}

#[test]
fn unknown_fields_are_ignored() {
    let with = check("pins").unwrap();
    let without = check("pins-no-extra").unwrap();
    assert!(
        String::from_utf8(fixture("pins").0)
            .unwrap()
            .contains("\"note\"")
    );
    assert_eq!(with.published(), without.published());
    assert_eq!(with.version(), without.version());
    assert_eq!(with.pin("chrome", "win64"), without.pin("chrome", "win64"));
    assert_eq!(
        with.pin("chrome-headless-shell", "linux64"),
        without.pin("chrome-headless-shell", "linux64")
    );
    // Not the same bytes, though: one does not replace the other.
    assert!(!accept(&without, Some(&with), None));
}

#[test]
fn a_bad_sha256_size_or_version_is_refused() {
    for (name, error) in [
        ("pins-sha-upper", PinsError::Pin),
        ("pins-sha-short", PinsError::Pin),
        ("pins-size-zero", PinsError::Pin),
        ("pins-size-over", PinsError::Pin),
        ("pins-version-bad", PinsError::Version),
    ] {
        assert_eq!(check(name), Err(error), "{name}");
    }
}

#[test]
fn a_manifest_over_the_size_bound_is_refused_before_its_signature() {
    let big = vec![b' '; usize::try_from(super::MAX_MANIFEST).unwrap() + 1];
    assert_eq!(
        Pins::verify(&big, b"not read", &[&test_key("a")]),
        Err(PinsError::TooBig)
    );
}

#[test]
fn no_rollback_takes_only_a_later_manifest_with_no_lower_version() {
    let pins = check("pins").unwrap();
    let newer = check("pins-newer").unwrap();
    let older = check("pins-older").unwrap();
    let lower = check("pins-lower").unwrap();
    let equal = check("pins-equal").unwrap();
    let other_bytes = check("pins-no-extra").unwrap();
    assert!(accept(&pins, None, None));
    assert!(accept(&newer, Some(&pins), None));
    assert!(!accept(&older, Some(&pins), None), "published earlier");
    assert!(
        !accept(&lower, Some(&pins), None),
        "a lower version, though later"
    );
    assert!(
        !accept(&other_bytes, Some(&pins), None),
        "equally dated, other bytes: not newer, not the same"
    );
    assert!(
        accept(&equal, Some(&pins), None),
        "the same manifest bytes under another signature"
    );
    assert!(!accept(&pins, Some(&newer), None));
    // The same bytes are taken; a heartbeat (same version, published later) too.
    assert!(accept(&check("pins").unwrap(), Some(&pins), None));
    assert!(accept(&check("pins-heartbeat").unwrap(), Some(&pins), None));
}

/// A source over `dir`'s `bundled` and `stored` folders, test key A.
fn source(dir: &TempDir) -> PinsSource {
    PinsSource {
        bundled: Some(dir.path().join("bundled")),
        stored: dir.path().join("stored"),
        keys: vec![test_key("a")],
    }
}

#[test]
fn the_newer_of_the_bundled_and_the_stored_copy_is_used() {
    let now = at("2026-11-09T00:00:00Z");
    let dir = TempDir::new("pins-newest");
    let source = source(&dir);
    assert_eq!(source.newest(now), None);
    put(&dir.path().join("bundled"), "pins");
    assert_eq!(source.newest(now).unwrap().run(), "1234");
    put(&dir.path().join("stored"), "pins-newer");
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.50");
    // A stored copy older than the bundled one (an app update shipped a newer one) is passed
    // over, and so is one pinning a lower version, though dated later.
    put(&dir.path().join("stored"), "pins-older");
    assert_eq!(source.newest(now).unwrap().run(), "1234");
    put(&dir.path().join("stored"), "pins-lower");
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.38");
    // The stored copy alone serves when the bundled one is missing or refused.
    put(&dir.path().join("stored"), "pins-newer");
    put(&dir.path().join("bundled"), "pins-by-c");
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.50");
}

#[test]
fn a_mismatched_pair_on_disk_is_not_read_as_good() {
    let now = at("2026-11-09T00:00:00Z");
    let dir = TempDir::new("pins-torn");
    let stored = dir.path().join("stored");
    put(&stored, "pins");
    // The crash case: the new body beside the old signature.
    let (newer, _) = fixture("pins-newer");
    write::write_file(&stored.join(MANIFEST_FILE), &newer).unwrap();
    let source = PinsSource {
        bundled: None,
        ..source(&dir)
    };
    assert_eq!(source.newest(now), None);
}

/// The clock set back: the newest copy dated over a day ahead makes the list unavailable
/// (`BrowserListUnavailable`), never the lower bundled copy.
#[test]
fn with_the_clock_set_back_a_copy_dated_ahead_leaves_no_list_not_the_lower_one() {
    let dir = TempDir::new("pins-ahead");
    let source = source(&dir);
    put(&dir.path().join("bundled"), "pins");
    put(&dir.path().join("stored"), "pins-ahead");
    // Published 2026-11-20: more than a day ahead of the 18th, not of the 19th.
    assert_eq!(source.newest(at("2026-11-18T23:59:59Z")), None);
    let pins = source.newest(at("2026-11-19T00:00:00Z")).unwrap();
    assert_eq!(pins.version(), "157.0.8103.60");
    // Set aside for use on the 18th, but the no-rollback floor whatever the date.
    let floor = source.floor().unwrap();
    assert_eq!(floor.version(), "157.0.8103.60");
    assert!(!accept(&check("pins-newer").unwrap(), Some(&floor), None));
    // The bundled copy alone, dated ahead, leaves no list either.
    let dir = TempDir::new("pins-ahead-bundled");
    put(&dir.path().join("bundled"), "pins-ahead");
    assert_eq!(self::source(&dir).newest(at("2026-11-18T00:00:00Z")), None);
}

#[test]
fn the_list_expires_after_45_days() {
    let published = Generated::parse("2026-11-07T06:00:00Z").unwrap();
    assert!(
        !expired(published, at("2026-12-22T06:00:00Z")),
        "45 days exactly"
    );
    assert!(expired(published, at("2026-12-22T06:00:01Z")));
    // From 14 to 45 days: old, not expired.
    assert_eq!(
        notice(published, at("2026-12-21T06:00:00Z")),
        Some(ToolNotice::BrowserListOld)
    );
    assert!(!expired(published, at("2026-12-21T06:00:00Z")));
}

#[test]
fn the_notice_says_the_list_is_old_after_14_days() {
    let published = Generated::parse("2026-11-07T06:00:00Z").unwrap();
    assert_eq!(notice(published, at("2026-11-21T06:00:00Z")), None);
    assert_eq!(
        notice(published, at("2026-11-21T06:00:01Z")),
        Some(ToolNotice::BrowserListOld)
    );
    assert_eq!(notice(published, at("2026-11-07T06:00:00Z")), None);
}

#[test]
fn stored_pairs_are_written_body_first_and_read_back() {
    let dir = TempDir::new("pins-store");
    let folder = dir.path().join("deep").join("browser-pins");
    let (body, signature) = fixture("pins-newer");
    let pins = check("pins-newer").unwrap();
    super::store(&folder, &pins, &body, &signature).unwrap();
    assert_eq!(std::fs::read(folder.join(MANIFEST_FILE)).unwrap(), body);
    assert_eq!(
        std::fs::read(folder.join(SIGNATURE_FILE)).unwrap(),
        signature
    );
}

#[test]
fn a_signature_that_does_not_verify_is_read_once_more() {
    let dir = TempDir::new("pins-reread");
    let source = source(&dir);
    let (newer, newer_signature) = fixture("pins-newer");
    let (_, old_signature) = fixture("pins");
    // The torn window: the new manifest beside the old signature, then the finished pair.
    let mut reads = 0;
    let pins = source.read_with("stored", &mut || {
        reads += 1;
        let signature = if reads == 1 {
            &old_signature
        } else {
            &newer_signature
        };
        Ok((newer.clone(), signature.clone()))
    });
    assert_eq!(pins.unwrap().version(), "157.0.8103.50");
    assert_eq!(reads, 2);
    // Still torn on the second read: refused, and not read a third time.
    let mut reads = 0;
    let pins = source.read_with("stored", &mut || {
        reads += 1;
        Ok((newer.clone(), old_signature.clone()))
    });
    assert_eq!(pins, None);
    assert_eq!(reads, 2);
    // Any other refusal is final at once.
    let mut reads = 0;
    let (body, signature) = fixture("pins-schema-2");
    let pins = source.read_with("stored", &mut || {
        reads += 1;
        Ok((body.clone(), signature.clone()))
    });
    assert_eq!(pins, None);
    assert_eq!(reads, 1);
}

#[test]
fn a_legacy_signature_is_refused() {
    // Signed over the manifest itself (minisign's legacy `Ed`), not its BLAKE2b hash.
    assert_eq!(
        check("pins-legacy"),
        Err(PinsError::Signature(Error::Signature))
    );
    // The same manifest, key and comment, prehashed: verifies. Only the kind differs.
    assert!(check("pins").is_ok());
    assert_eq!(fixture("pins-legacy").0, fixture("pins").0);
}

mod floor;
