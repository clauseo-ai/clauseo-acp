//! `SetDefaultLocation` over the wire (the app's move of the matters folder out of a protected
//! folder such as Documents): a picked folder becomes `matters.default_location` and every app
//! subscriber hears it; a policy that sets the key refuses it with `settings.locked`; a folder
//! ref the host did not issue, this run or an earlier one, is `command.invalid`; and an existing
//! matter keeps its folder.

use clauseo_protocol::{
    CommandOutcome, ErrorCode, FolderRefId, FolderState, HostCommand, HostEvent, HostQuery,
    MatterId, MatterSource, QueryResult, Title,
};
use uuid::Uuid;

use crate::matters::{created, summary};
use crate::settings::{ready, settings, subscribe_app};
use crate::support::{Dir, Running};

fn set_default_location(folder: FolderRefId) -> HostCommand {
    HostCommand::SetDefaultLocation { folder }
}

/// `GetDefaultLocation`'s display.
fn default_display(host: &Running) -> String {
    match host.query(HostQuery::GetDefaultLocation).unwrap() {
        QueryResult::DefaultLocation(location) => location.display,
        other => panic!("GetDefaultLocation answered {other:?}"),
    }
}

/// `settings.json`'s text, or `None` when the host has not written one.
fn saved_text(dir: &Dir) -> Option<String> {
    std::fs::read_to_string(dir.data().join("settings.json")).ok()
}

/// A matter made in the default location, by `DefaultLocation`.
fn create_in_default_location(host: &Running, name: &str) -> MatterId {
    created(host.dispatch(HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source: MatterSource::DefaultLocation,
        client: None,
        next_date: None,
    }))
}

#[test]
fn a_picked_folder_becomes_the_default_location_and_settings_hear_it() {
    let dir = Dir::new("default_location_picked", &[]);
    let host = dir.start(0);
    let (mut sub, _) = subscribe_app(&host);
    let folder = dir.folder("Matters", &[]);
    let folder_ref = host.folder_ref(&folder);
    assert_eq!(
        host.ok(set_default_location(folder_ref)),
        CommandOutcome::Done
    );
    assert_eq!(default_display(&host), folder.display().to_string());
    host.settle();
    let changed = ready(&mut sub);
    assert!(
        matches!(&changed[..], [HostEvent::SettingsChanged(_)]),
        "{changed:?}"
    );
    // The setting is saved, and a default create makes its matter in the picked folder.
    let saved: serde_json::Value = serde_json::from_str(&saved_text(&dir).unwrap()).unwrap();
    assert_eq!(
        saved["matters"]["default_location"],
        folder.display().to_string()
    );
    let matter = create_in_default_location(&host, "Acme");
    assert!(
        folder
            .join("Acme")
            .join(".clauseo")
            .join("matter.json")
            .is_file()
    );
    assert_eq!(summary(&host, matter).folder_state, FolderState::Found);
    host.shutdown();

    // A restart reads the same location back.
    let host = dir.start(1);
    assert_eq!(default_display(&host), folder.display().to_string());
    host.shutdown();
}

#[test]
fn a_policy_that_sets_the_default_location_refuses_the_change_and_changes_nothing() {
    let dir = Dir::new("default_location_locked", &[]);
    let host = dir.start_with_policy(0, r#"{"matters": {"default_location": null}}"#);
    assert!(
        settings(&host)
            .locked
            .iter()
            .any(|key| key == "matters.default_location")
    );
    let (mut sub, _) = subscribe_app(&host);
    let before = default_display(&host);
    let saved_before = saved_text(&dir);
    let folder = dir.folder("Matters", &[]);
    let folder_ref = host.folder_ref(&folder);
    let error = host.refused(set_default_location(folder_ref), ErrorCode::SettingsLocked);
    assert!(
        error.message().contains("matters.default_location"),
        "{error}"
    );
    assert_eq!(default_display(&host), before);
    assert_eq!(saved_text(&dir), saved_before);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn a_folder_ref_the_host_did_not_issue_is_refused_as_invalid() {
    let dir = Dir::new("default_location_unknown", &[]);
    let host = dir.start(0);
    let folder = dir.folder("Matters", &[]);
    // A ref from an earlier run is stale: this run never issued it.
    let stale = host.folder_ref(&folder);
    host.shutdown();
    let host = dir.start(1);
    let before = default_display(&host);
    let saved_before = saved_text(&dir);
    let never_issued = FolderRefId::new(Uuid::now_v7()).unwrap();
    for folder in [never_issued, stale] {
        let error = host.refused(set_default_location(folder), ErrorCode::CommandInvalid);
        assert!(error.message().contains("folder ref"), "{error}");
    }
    assert_eq!(default_display(&host), before);
    assert_eq!(saved_text(&dir), saved_before);
    host.shutdown();
}

#[test]
fn an_existing_matter_keeps_its_folder_and_nothing_is_made_or_moved() {
    let dir = Dir::new("default_location_existing", &[]);
    let host = dir.start(0);
    let old = dir.folder("Acme", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("Acme", &old);
    let listed = summary(&host, matter);
    let location = dir.folder("Matters", &[("keep.txt", "y\n")]);
    let location_ref = host.folder_ref(&location);
    host.ok(set_default_location(location_ref));
    // The matter is where it was, found, and its folder holds what it held.
    assert_eq!(summary(&host, matter), listed);
    assert!(old.join(".clauseo").join("matter.json").is_file());
    // The picked folder gained nothing: no matter folder and no `.clauseo`.
    assert!(!location.join(".clauseo").exists());
    assert!(!location.join("Acme").exists());
    assert_eq!(std::fs::read_dir(&location).unwrap().count(), 1);
    host.shutdown();
}
