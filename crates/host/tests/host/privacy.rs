//! The privacy switches over the wire (decision record 0036): `SettingsView.privacy`,
//! `SetPrivacySettings`, `FirstAct`, the policy lock that only turns counts off, and the rule
//! that off is saved first while on is refused until the old counts are gone.

use clauseo_counts::Mode;
use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, HostEvent, PrivacyPatch, PrivacySettings,
};
use serde_json::Value;

use crate::counts::{open_counts, start_counting};
use crate::settings::{ready, settings, subscribe_app};
use crate::support::{Dir, Running};

/// The privacy switches' defaults, as `clauseo-settings` has them, before any first act.
pub(crate) fn privacy_defaults() -> PrivacySettings {
    PrivacySettings {
        counts: true,
        crash_reports: false,
        counts_started: false,
    }
}

fn set_counts(on: bool) -> HostCommand {
    HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            counts: Some(on),
            crash_reports: None,
        },
    }
}

/// `settings.json`'s `privacy` section.
fn saved_privacy(dir: &Dir) -> Value {
    let saved: Value = serde_json::from_str(&dir.settings_json()).unwrap();
    saved["privacy"].clone()
}

fn marker(dir: &Dir) -> bool {
    dir.data().join("first_act_done").exists()
}

fn privacy(host: &Running) -> PrivacySettings {
    settings(host).privacy
}

#[test]
fn the_view_shows_counts_on_but_not_started_until_the_first_act() {
    let dir = Dir::new("privacy_defaults", &[]);
    let host = dir.start(0);
    assert_eq!(privacy(&host), privacy_defaults());
    host.shutdown();
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 1, &counts);
    assert_eq!(privacy(&host), privacy_defaults());
    host.shutdown();
}

#[test]
fn first_act_starts_counts_and_a_second_one_changes_nothing() {
    let dir = Dir::new("privacy_first_act", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    let (mut sub, _) = subscribe_app(&host);
    assert_eq!(counts.mode(), Mode::Memory);
    assert_eq!(
        host.ok(HostCommand::FirstAct { counts: true }),
        CommandOutcome::Done
    );
    assert!(marker(&dir));
    assert_eq!(counts.mode(), Mode::Saving);
    let id = counts.install_id().unwrap();
    assert!(dir.data().join("counts").join("install_id").exists());
    let started = PrivacySettings {
        counts_started: true,
        ..privacy_defaults()
    };
    assert_eq!(privacy(&host), started);
    host.settle();
    let changed = ready(&mut sub);
    assert!(
        matches!(&changed[..], [HostEvent::SettingsChanged(view)] if view.privacy == started),
        "{changed:?}"
    );

    // A second first act, even one turning counts off, changes nothing and answers Done.
    assert_eq!(
        host.ok(HostCommand::FirstAct { counts: false }),
        CommandOutcome::Done
    );
    assert_eq!(counts.mode(), Mode::Saving);
    assert_eq!(counts.install_id(), Some(id));
    assert_eq!(privacy(&host), started);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn first_act_with_counts_off_saves_off_and_keeps_nothing() {
    let dir = Dir::new("privacy_first_act_off", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    host.ok(HostCommand::FirstAct { counts: false });
    assert!(marker(&dir));
    assert_eq!(counts.mode(), Mode::Off);
    assert!(!dir.data().join("counts").exists());
    assert_eq!(saved_privacy(&dir), serde_json::json!({"counts": false}));
    assert_eq!(
        privacy(&host),
        PrivacySettings {
            counts: false,
            crash_reports: false,
            counts_started: true,
        }
    );
    host.shutdown();
}

#[test]
fn a_patch_naming_counts_before_the_marker_is_the_first_act() {
    let dir = Dir::new("privacy_patch_first_act", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    // Crash reports alone are no first act.
    host.ok(HostCommand::SetPrivacySettings {
        patch: PrivacyPatch {
            crash_reports: Some(true),
            counts: None,
        },
    });
    assert!(!marker(&dir));
    assert_eq!(counts.mode(), Mode::Memory);
    host.ok(set_counts(true));
    assert!(marker(&dir));
    assert_eq!(counts.mode(), Mode::Saving);
    assert!(privacy(&host).counts_started);
    host.shutdown();
}

#[test]
fn a_policy_false_locks_counts_off_and_a_patch_naming_it_is_refused() {
    let dir = Dir::new("privacy_policy_off", &[]);
    dir.set_policy(r#"{"privacy": {"counts": false}}"#);
    let counts = open_counts(&dir, false);
    let host = start_counting(&dir, 0, &counts);
    assert!(!privacy(&host).counts);
    assert!(
        settings(&host)
            .locked
            .contains(&"privacy.counts".to_owned())
    );
    for on in [true, false] {
        host.refused(set_counts(on), ErrorCode::SettingsLocked);
    }
    assert!(!dir.data().join("settings.json").exists() || saved_privacy(&dir).is_null());
    assert!(!marker(&dir));
    // The welcome's first act still answers Done; policy's off stands over the switch.
    assert_eq!(
        host.ok(HostCommand::FirstAct { counts: true }),
        CommandOutcome::Done
    );
    assert!(marker(&dir));
    assert_eq!(counts.mode(), Mode::Off);
    assert!(!privacy(&host).counts);
    host.shutdown();
}

#[test]
fn a_policy_true_is_ignored_and_the_switch_stays_the_lawyers() {
    let dir = Dir::new("privacy_policy_on", &[]);
    dir.set_settings(&[], serde_json::json!({"privacy": {"counts": false}}));
    dir.set_policy(r#"{"privacy": {"counts": true}}"#);
    let host = dir.start(0);
    assert!(!privacy(&host).counts);
    assert!(
        !settings(&host)
            .locked
            .contains(&"privacy.counts".to_owned())
    );
    host.ok(set_counts(true));
    assert!(privacy(&host).counts);
    host.shutdown();
}

// The 0036 rule. Unix only: a folder whose mode forbids changes is how a delete is made to
// fail here; CI runs every other test on Windows too.
#[cfg(unix)]
#[test]
fn off_is_saved_when_the_folder_cannot_be_deleted_and_on_is_refused_until_it_can() {
    use crate::write;

    let dir = Dir::new("privacy_undeletable", &[]);
    let counts = open_counts(&dir, true);
    let host = start_counting(&dir, 0, &counts);
    host.ok(HostCommand::FirstAct { counts: true });
    let old = counts.install_id().unwrap();
    let folder = dir.data().join("counts");
    assert!(folder.join("install_id").exists());
    // Not as root, which ignores the mode (CI and the build machines run tests as a user).
    write::set_mode(&folder, 0o555);

    // Off is saved and counts stop, though the folder stays.
    assert_eq!(host.ok(set_counts(false)), CommandOutcome::Done);
    assert_eq!(saved_privacy(&dir), serde_json::json!({"counts": false}));
    assert_eq!(counts.mode(), Mode::Off);
    assert!(folder.join("install_id").exists());
    assert!(!privacy(&host).counts);

    // On is refused while the old counts cannot go, and the setting stays false.
    host.refused(set_counts(true), ErrorCode::StoreIo);
    assert_eq!(saved_privacy(&dir), serde_json::json!({"counts": false}));
    assert_eq!(counts.mode(), Mode::Off);
    assert!(!privacy(&host).counts);

    // Once the folder can go, on deletes it and starts with a new id.
    write::set_mode(&folder, 0o755);
    assert_eq!(host.ok(set_counts(true)), CommandOutcome::Done);
    assert_eq!(counts.mode(), Mode::Saving);
    assert_ne!(counts.install_id(), Some(old));
    assert!(privacy(&host).counts);
    host.shutdown();
}

#[test]
fn the_privacy_switches_mirror_the_settings_crate() {
    let settings_privacy =
        serde_json::to_value(clauseo_settings::PrivacySettings::default()).unwrap();
    let wire_privacy = serde_json::to_value(privacy_defaults()).unwrap();
    // The wire adds `counts_started`, which is the counts' state, not a setting.
    let mut wire_settings = wire_privacy.as_object().unwrap().clone();
    assert_eq!(
        wire_settings.remove("counts_started"),
        Some(Value::Bool(false))
    );
    assert_eq!(Value::Object(wire_settings), settings_privacy);
    let settings_patch = clauseo_settings::PrivacyPatch {
        counts: Some(false),
        crash_reports: Some(true),
    };
    let wire_patch = PrivacyPatch {
        counts: Some(false),
        crash_reports: Some(true),
    };
    assert_eq!(
        serde_json::to_value(settings_patch).unwrap(),
        serde_json::to_value(wire_patch).unwrap()
    );
}
