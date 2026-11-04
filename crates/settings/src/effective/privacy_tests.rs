//! `privacy.counts` and `privacy.crash_reports` (decision record 0036): the defaults, a policy
//! that can only turn them off, the fail-closed rule for an unreadable source,
//! `Effective::policy_present`, and `Effective::apply_privacy_patch`.

use super::Effective;
use crate::error::Error;
use crate::policy::PolicyRead;
use crate::privacy::PrivacyPatch;
use crate::schema::PrivacySettings;
use crate::testing::{FakeSource, MemFiles, abs, locations, policy, user};
use crate::warning::{Origin, WarningKind};

const PRIVACY: [&str; 2] = ["privacy.counts", "privacy.crash_reports"];

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

fn absent() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "absent",
        read: || PolicyRead::Absent,
    }
}

/// `settings.json` with both switches on.
fn both_on() -> MemFiles {
    user(r#"{"privacy": {"counts": true, "crash_reports": true}}"#)
}

fn both(counts: bool, crash_reports: bool) -> PrivacySettings {
    PrivacySettings {
        counts,
        crash_reports,
    }
}

/// The warnings about `key`.
fn warnings_about(effective: &Effective, key: &str) -> Vec<String> {
    effective
        .warnings()
        .iter()
        .filter_map(|warning| match &warning.kind {
            WarningKind::InvalidValue { key: name, message } if name == key => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn counts_default_on_and_crash_reports_off_unlocked() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    assert_eq!(effective.settings().privacy, both(true, false));
    for key in PRIVACY {
        assert!(!effective.is_locked(key), "{key}");
    }
}

#[test]
fn the_users_switches_load_unlocked() {
    let files = user(r#"{"privacy": {"counts": false, "crash_reports": true}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.warnings(), []);
    assert_eq!(effective.settings().privacy, both(false, true));
    assert_eq!(effective.locked().count(), 0);
}

#[test]
fn an_unreadable_policy_source_locks_both_off() {
    let effective = Effective::load(&locations(), &both_on(), &[&unreadable()]);
    assert_eq!(effective.settings().privacy, both(false, false));
    for key in PRIVACY {
        assert!(effective.is_locked(key), "{key}");
        assert_eq!(warnings_about(&effective, key).len(), 1, "{key}");
    }
}

#[test]
fn a_policy_false_locks_the_key_off() {
    let firm = policy(
        "firm",
        r#"{"privacy": {"counts": false, "crash_reports": false}}"#,
    );
    let effective = Effective::load(&locations(), &both_on(), &[&firm]);
    assert_eq!(effective.settings().privacy, both(false, false));
    for key in PRIVACY {
        assert!(effective.is_locked(key), "{key}");
    }
    assert_eq!(effective.warnings(), []);
}

// A policy can only turn the privacy keys off (0036): `true` is ignored, warned about, and
// leaves the lawyer's own switch in force and unlocked.
#[test]
fn a_policy_true_is_ignored_with_a_warning_and_does_not_lock() {
    let files = user(r#"{"privacy": {"counts": false, "crash_reports": false}}"#);
    let firm = policy(
        "firm",
        r#"{"privacy": {"counts": true, "crash_reports": true}}"#,
    );
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(effective.settings().privacy, both(false, false));
    for key in PRIVACY {
        assert!(!effective.is_locked(key), "{key}");
        let warnings = warnings_about(&effective, key);
        assert_eq!(warnings.len(), 1, "{key}: {warnings:?}");
        assert!(warnings[0].contains("only turn"), "{}", warnings[0]);
    }
    let origins: Vec<_> = effective.warnings().iter().map(|w| &w.origin).collect();
    assert!(
        origins
            .iter()
            .all(|origin| matches!(origin, Origin::Store { name } if name == "firm"))
    );
    // The lawyer can still change an ignored key.
    let mut effective = effective;
    effective
        .apply_privacy_patch(
            &files,
            &PrivacyPatch {
                counts: Some(true),
                crash_reports: None,
            },
        )
        .unwrap();
    assert!(effective.settings().privacy.counts);
}

// A stronger source's ignored `true` takes no key, so a weaker source's `false` still locks.
#[test]
fn a_weaker_policy_false_locks_after_a_stronger_true_is_ignored() {
    let strong = policy("strong", r#"{"privacy": {"counts": true}}"#);
    let weak = policy("weak", r#"{"privacy": {"counts": false}}"#);
    let effective = Effective::load(&locations(), &both_on(), &[&strong, &weak]);
    assert!(!effective.settings().privacy.counts);
    assert!(effective.is_locked("privacy.counts"));
}

#[test]
fn a_policy_value_of_the_wrong_type_locks_the_key_off() {
    let firm = policy(
        "firm",
        r#"{"privacy": {"counts": "yes", "crash_reports": 1}}"#,
    );
    let effective = Effective::load(&locations(), &both_on(), &[&firm]);
    assert_eq!(effective.settings().privacy, both(false, false));
    for key in PRIVACY {
        assert!(effective.is_locked(key), "{key}");
    }
}

#[test]
fn a_misnamed_privacy_section_locks_both_off() {
    let firm = policy("firm", r#"{"privacy": "off"}"#);
    let effective = Effective::load(&locations(), &both_on(), &[&firm]);
    assert_eq!(effective.settings().privacy, both(false, false));
    for key in PRIVACY {
        assert!(effective.is_locked(key), "{key}");
    }
}

#[test]
fn policy_present_is_false_with_no_policy_source() {
    let none = Effective::load(&locations(), &both_on(), &[]);
    assert!(!none.policy_present());
    let only_absent = Effective::load(&locations(), &both_on(), &[&absent()]);
    assert!(!only_absent.policy_present());
}

#[test]
fn policy_present_is_true_for_a_policy_silent_on_privacy() {
    let firm = policy("firm", r#"{"ui": {"theme": "dark"}}"#);
    let effective = Effective::load(&locations(), &both_on(), &[&absent(), &firm]);
    assert!(effective.policy_present());
    assert_eq!(effective.settings().privacy, both(true, true));
    for key in PRIVACY {
        assert!(!effective.is_locked(key), "{key}");
    }
    // Present though it sets nothing this version knows.
    let empty = policy("empty", "{}");
    assert!(Effective::load(&locations(), &both_on(), &[&empty]).policy_present());
}

#[test]
fn policy_present_is_true_for_an_unreadable_source() {
    let effective = Effective::load(&locations(), &both_on(), &[&unreadable()]);
    assert!(effective.policy_present());
    assert!(effective.policy().is_absent());
}

#[test]
fn apply_privacy_patch_writes_then_sets_memory_and_a_fresh_load_agrees() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    let patch = PrivacyPatch {
        counts: Some(false),
        crash_reports: Some(true),
    };
    effective.apply_privacy_patch(&files, &patch).unwrap();
    assert_eq!(effective.settings().privacy, both(false, true));
    assert_eq!(
        files.text(locations().settings_file()).unwrap(),
        "{\n  \"privacy\": {\n    \"counts\": false,\n    \"crash_reports\": true\n  }\n}\n"
    );
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.settings().privacy, effective.settings().privacy);
}

#[test]
fn a_patch_naming_a_locked_key_is_refused_whole() {
    let files = both_on();
    let firm = policy("firm", r#"{"privacy": {"counts": false}}"#);
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let before = files.text(locations().settings_file());
    let patch = PrivacyPatch {
        counts: Some(false),
        crash_reports: Some(false),
    };
    let error = effective.check_privacy_patch(&patch).unwrap_err();
    assert!(matches!(&error, Error::Locked { key } if key == "privacy.counts"));
    let error = effective.apply_privacy_patch(&files, &patch).unwrap_err();
    assert!(matches!(&error, Error::Locked { key } if key == "privacy.counts"));
    assert_eq!(files.text(locations().settings_file()), before);
    assert!(effective.settings().privacy.crash_reports);
}

#[test]
fn an_empty_patch_writes_nothing_and_a_failed_write_changes_nothing() {
    let files = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .apply_privacy_patch(&files, &PrivacyPatch::default())
        .unwrap();
    let error = effective
        .apply_privacy_patch(
            &files,
            &PrivacyPatch {
                counts: Some(false),
                crash_reports: None,
            },
        )
        .unwrap_err();
    assert!(matches!(error, Error::Io { .. }));
    assert_eq!(effective.settings().privacy, both(true, false));
}

#[test]
fn a_privacy_patch_refuses_other_keys() {
    for json in [
        r#"{"theme": "dark"}"#,
        r#"{"counts": "no"}"#,
        r#"{"web_search": true}"#,
    ] {
        assert!(
            serde_json::from_str::<PrivacyPatch>(json).is_err(),
            "{json}"
        );
    }
}
