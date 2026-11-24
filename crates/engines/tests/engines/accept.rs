//! The accept rules: the app listed, the rollback rule, never older than the bundled pins.

use clauseo_engines::{Context, Error, load_with_key};
use serde_json::json;

use crate::fixtures::{GOOD, KEY_A, NEWER, NOT_SEMVER, OLDER, Signed, bundled, context, refusal};

fn with(signed: &Signed, cx: &Context<'_>) -> Result<u64, Error> {
    load_with_key(signed.json, signed.sig, cx, KEY_A).map(|list| list.seq())
}

#[test]
fn an_app_not_in_apps_is_refused() {
    let bundled = bundled();
    for version in ["0.2.0", "0.1", "0.1.0 ", "v0.1.0", "0.0.0-dev.1a2b3c"] {
        let cx = Context {
            app_version: version,
            ..context(&bundled)
        };
        assert_eq!(with(&GOOD, &cx), Err(Error::AppNotListed), "{version:?}");
    }
}

#[test]
fn a_dev_build_listed_by_its_exact_version_is_accepted() {
    let bundled = bundled();
    let cx = Context {
        app_version: "0.0.0-dev.1a2b3c4",
        ..context(&bundled)
    };
    assert_eq!(with(&GOOD, &cx), Ok(2));
}

#[test]
fn a_seq_not_above_the_one_in_use_is_refused() {
    let bundled = bundled();
    for in_use in [2, 3, u64::MAX] {
        let cx = Context {
            in_use_seq: Some(in_use),
            ..context(&bundled)
        };
        assert_eq!(
            with(&GOOD, &cx),
            Err(Error::NotNewer {
                seq: 2,
                floor: in_use
            })
        );
    }
    let cx = Context {
        in_use_seq: Some(2),
        ..context(&bundled)
    };
    assert_eq!(with(&NEWER, &cx), Ok(3));
}

#[test]
fn a_seq_not_above_the_bundled_one_is_refused() {
    let bundled = bundled();
    let cx = Context {
        bundled_seq: 2,
        ..context(&bundled)
    };
    assert_eq!(with(&GOOD, &cx), Err(Error::NotNewer { seq: 2, floor: 2 }));
    // The larger of the two is the floor, whichever it is.
    let cx = Context {
        bundled_seq: 3,
        in_use_seq: Some(1),
        ..context(&bundled)
    };
    assert_eq!(with(&NEWER, &cx), Err(Error::NotNewer { seq: 3, floor: 3 }));
}

#[test]
fn a_version_older_than_the_bundled_one_is_refused() {
    // The list's codex override is 0.162.0; the bundle pins 0.162.1.
    assert_eq!(refusal(&OLDER), Some(Error::OlderThanBundled));
    // A bundle newer than the good list's claude-agent-acp.
    let mut newer_bundle = bundled();
    newer_bundle["dependencies"]["@agentclientprotocol/claude-agent-acp"] = json!("0.89.2");
    assert_eq!(
        with(&GOOD, &context(&newer_bundle)),
        Err(Error::OlderThanBundled)
    );
    // A bundle a patch ahead of the list.
    let mut released = bundled();
    released["dependencies"]["@agentclientprotocol/codex-acp"] = json!("2.2.3");
    assert_eq!(
        with(&GOOD, &context(&released)),
        Err(Error::OlderThanBundled)
    );
    // Equal versions are accepted (only the seq must move).
    assert_eq!(with(&GOOD, &context(&bundled())), Ok(2));
}

#[test]
fn a_bundle_without_the_override_does_not_need_it() {
    let mut bundle = bundled();
    bundle.as_object_mut().unwrap().remove("overrides");
    assert_eq!(with(&GOOD, &context(&bundle)), Ok(2));
}

#[test]
fn a_version_that_is_not_plain_semver_is_refused() {
    assert_eq!(refusal(&NOT_SEMVER), Some(Error::Version));
    let mut range = bundled();
    range["dependencies"]["@agentclientprotocol/codex-acp"] = json!("^2.2.2");
    assert_eq!(with(&GOOD, &context(&range)), Err(Error::BundledVersion));
    let mut missing = bundled();
    missing["dependencies"]
        .as_object_mut()
        .unwrap()
        .remove("@agentclientprotocol/codex-acp");
    assert_eq!(with(&GOOD, &context(&missing)), Err(Error::BundledVersion));
}
