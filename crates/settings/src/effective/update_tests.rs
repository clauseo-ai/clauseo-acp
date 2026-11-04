//! `update.mode` (task R23): the default, the user's value, policy locking it at any of its
//! three values, the fail-closed rule for an unreadable source, and
//! `Effective::apply_update_mode`.

use super::Effective;
use crate::error::Error;
use crate::policy::PolicyRead;
use crate::schema::UpdateMode;
use crate::testing::{FakeSource, MemFiles, abs, locations, policy, user};
use crate::update::UpdateChoice;
use crate::warning::{Origin, WarningKind};
use std::path::Path;

const KEY: &str = "update.mode";

fn unreadable() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "broken",
        read: || {
            PolicyRead::Unreadable(Error::Parse {
                path: abs("policy.json"),
                line: 1,
                column: 1,
                message: "expected value".to_owned(),
            })
        },
    }
}

/// The `InvalidValue` warnings about `update.mode`, with the store each came from.
fn warnings(effective: &Effective) -> Vec<(Origin, String)> {
    effective
        .warnings()
        .iter()
        .filter_map(|warning| match &warning.kind {
            WarningKind::InvalidValue { key, message } if key == KEY => {
                Some((warning.origin.clone(), message.clone()))
            }
            _ => None,
        })
        .collect()
}

fn mode(effective: &Effective) -> UpdateMode {
    effective.settings().update.mode
}

#[test]
fn the_mode_defaults_to_auto_unlocked() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    assert_eq!(mode(&effective), UpdateMode::Auto);
    assert_eq!(UpdateMode::default(), UpdateMode::Auto);
    assert!(!effective.is_locked(KEY));
}

#[test]
fn the_users_mode_is_read_unlocked() {
    for (text, expected) in [
        ("none", UpdateMode::None),
        ("notify", UpdateMode::Notify),
        ("auto", UpdateMode::Auto),
    ] {
        let files = user(&format!(r#"{{"update": {{"mode": "{text}"}}}}"#));
        let effective = Effective::load(&locations(), &files, &[]);
        assert_eq!(effective.warnings(), [], "{text}");
        assert_eq!(mode(&effective), expected, "{text}");
        assert!(!effective.is_locked(KEY), "{text}");
    }
}

#[test]
fn a_bad_user_mode_is_a_warning_and_the_default_applies() {
    for value in [r#""off""#, r#""Notify""#, "true", "1", "null"] {
        let files = user(&format!(r#"{{"update": {{"mode": {value}}}}}"#));
        let effective = Effective::load(&locations(), &files, &[]);
        assert_eq!(mode(&effective), UpdateMode::Auto, "{value}");
        assert_eq!(warnings(&effective).len(), 1, "{value}");
        assert!(!effective.is_locked(KEY), "{value}");
    }
}

#[test]
fn a_policy_none_locks_the_mode_at_none() {
    let files = user(r#"{"update": {"mode": "notify"}}"#);
    let firm = policy("firm", r#"{"update": {"mode": "none"}}"#);
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(mode(&effective), UpdateMode::None);
    assert!(effective.is_locked(KEY));
    assert_eq!(effective.warnings(), []);
}

// Not an off-only key: a policy may set any of the three values, and each locks.
#[test]
fn a_policy_notify_or_auto_locks_the_mode_at_that_value() {
    let files = user(r#"{"update": {"mode": "none"}}"#);
    for (text, expected) in [("notify", UpdateMode::Notify), ("auto", UpdateMode::Auto)] {
        let firm = policy("firm", format!(r#"{{"update": {{"mode": "{text}"}}}}"#));
        let effective = Effective::load(&locations(), &files, &[&firm]);
        assert_eq!(mode(&effective), expected, "{text}");
        assert!(effective.is_locked(KEY), "{text}");
        assert_eq!(effective.warnings(), [], "{text}");
    }
}

// A security key fails closed: a policy value that does not type locks it at `none`, with a
// warning, rather than at its default `auto`.
#[test]
fn a_bad_policy_mode_locks_the_mode_at_none_with_a_warning() {
    let files = user(r#"{"update": {"mode": "auto"}}"#);
    for value in [r#""off""#, r#""Auto""#, "false", "0"] {
        let firm = policy("firm", format!(r#"{{"update": {{"mode": {value}}}}}"#));
        let effective = Effective::load(&locations(), &files, &[&firm]);
        assert_eq!(mode(&effective), UpdateMode::None, "{value}");
        assert!(effective.is_locked(KEY), "{value}");
        let found = warnings(&effective);
        assert_eq!(found.len(), 1, "{value}: {found:?}");
        assert!(
            matches!(&found[0].0, Origin::File { path, .. } if path == Path::new("firm")),
            "{found:?}"
        );
    }
}

#[test]
fn an_unreadable_policy_source_locks_the_mode_at_none() {
    let files = user(r#"{"update": {"mode": "notify"}}"#);
    let effective = Effective::load(&locations(), &files, &[&unreadable()]);
    assert_eq!(mode(&effective), UpdateMode::None);
    assert!(effective.is_locked(KEY));
    let found = warnings(&effective);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].0,
        Origin::Store {
            name: "broken".to_owned()
        }
    );
}

// The stronger source's value stands; the weaker unreadable one locks nothing it already set.
#[test]
fn a_stronger_policy_mode_stands_over_a_weaker_unreadable_source() {
    let firm = policy("firm", r#"{"update": {"mode": "notify"}}"#);
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&firm, &unreadable()]);
    assert_eq!(mode(&effective), UpdateMode::Notify);
    assert!(effective.is_locked(KEY));
}

#[test]
fn writing_the_mode_while_policy_locks_it_is_refused_and_writes_nothing() {
    let files = user(r#"{"update": {"mode": "auto"}}"#);
    let before = files.text(locations().settings_file());
    for source in ["firm", "broken"] {
        let firm = policy("firm", r#"{"update": {"mode": "none"}}"#);
        let broken = unreadable();
        let mut effective = match source {
            "firm" => Effective::load(&locations(), &files, &[&firm]),
            _ => Effective::load(&locations(), &files, &[&broken]),
        };
        for choice in [UpdateChoice::Auto, UpdateChoice::Notify] {
            let error = effective.apply_update_mode(&files, choice).unwrap_err();
            assert!(
                matches!(&error, Error::Locked { key } if key == KEY),
                "{source}: {error:?}"
            );
            assert_eq!(files.text(locations().settings_file()), before, "{source}");
            assert_eq!(mode(&effective), UpdateMode::None, "{source}");
        }
    }
}

#[test]
fn writing_notify_persists_and_a_fresh_load_reads_it() {
    let files = user(r#"{"ui": {"theme": "dark"}, "update": {"mode": "none"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .apply_update_mode(&files, UpdateChoice::Notify)
        .unwrap();
    assert_eq!(mode(&effective), UpdateMode::Notify);
    assert_eq!(
        files.text(locations().settings_file()).unwrap(),
        "{\n  \"ui\": {\n    \"theme\": \"dark\"\n  },\n  \"update\": {\n    \"mode\": \"notify\"\n  }\n}\n"
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(mode(&fresh), UpdateMode::Notify);
    effective
        .apply_update_mode(&files, UpdateChoice::Auto)
        .unwrap();
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(mode(&fresh), UpdateMode::Auto);
}

#[test]
fn a_failed_write_changes_nothing() {
    let files = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    let mut effective = Effective::load(&locations(), &files, &[]);
    let error = effective
        .apply_update_mode(&files, UpdateChoice::Notify)
        .unwrap_err();
    assert!(matches!(error, Error::Io { .. }), "{error:?}");
    assert_eq!(mode(&effective), UpdateMode::Auto);
}

// The app can never write `none`: the choice has no such value, on the wire or in Rust.
#[test]
fn the_choice_is_auto_or_notify_only() {
    for (text, choice) in [
        ("\"auto\"", UpdateChoice::Auto),
        ("\"notify\"", UpdateChoice::Notify),
    ] {
        assert_eq!(serde_json::from_str::<UpdateChoice>(text).unwrap(), choice);
        assert_eq!(serde_json::to_string(&choice).unwrap(), text);
    }
    for text in ["\"none\"", "\"Auto\"", "null", "true"] {
        assert!(
            serde_json::from_str::<UpdateChoice>(text).is_err(),
            "{text}"
        );
    }
}
