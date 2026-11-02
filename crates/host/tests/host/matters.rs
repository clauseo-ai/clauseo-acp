//! The matter commands of the rail at the interface: a create's details and its default
//! location, `UpdateMatter`, `OpenMatter`, `GetDefaultLocation`, and `ListMatters` with every new
//! field, across a restart. Forgetting is in `forget.rs`.

use std::path::Path;
use std::time::Duration;

use clauseo_host::Clock as _;
use clauseo_protocol::{
    ClientName, CommandOutcome, Date, ErrorCode, FieldChange, FolderState, HostCommand, HostQuery,
    MatterId, MatterKind, MatterSource, MatterStatus, MatterSummary, StopReason, StreamId, Title,
};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, send_prompt, wait};

pub(crate) fn client(name: &str) -> ClientName {
    ClientName::new(name).unwrap()
}

pub(crate) fn date(text: &str) -> Date {
    text.parse().unwrap()
}

/// The summary of `matter` in `ListMatters`.
pub(crate) fn summary(host: &Running, matter: MatterId) -> MatterSummary {
    host.matters()
        .into_iter()
        .find(|summary| summary.matter == matter)
        .unwrap_or_else(|| panic!("{matter} is not listed"))
}

/// Registers `folder` and imports it with the given details.
pub(crate) fn import(
    host: &Running,
    name: &str,
    folder: &Path,
    details: (Option<ClientName>, Option<Date>),
) -> Result<CommandOutcome, clauseo_protocol::HostError> {
    let folder = wait(
        "register_folder",
        host.handle.register_folder(folder.to_owned()),
    )
    .unwrap();
    host.dispatch(HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source: MatterSource::ExistingFolder { folder },
        client: details.0,
        next_date: details.1,
    })
}

pub(crate) fn created(outcome: Result<CommandOutcome, clauseo_protocol::HostError>) -> MatterId {
    match outcome {
        Ok(CommandOutcome::MatterCreated { matter }) => matter,
        other => panic!("CreateMatter answered {other:?}"),
    }
}

#[test]
fn a_create_keeps_its_details_and_list_matters_shows_every_field() {
    let dir = Dir::new("create_details", &[]);
    // The home holds the work folder, so the matter's folder shows below it.
    let home = dir.work().parent().unwrap().to_owned();
    let work = Path::new(dir.work().file_name().unwrap());
    let host = dir.start_with(0, |config| config.with_home_folder(Some(home.clone())));
    let folder = dir.folder("State v. Kunal Desai", &[("fir.txt", "x\n")]);
    let now = host.clock.now().wall;
    let matter = created(import(
        &host,
        "State v. Kunal Desai",
        &folder,
        (Some(client(" Kunal Desai ")), Some(date("2026-11-20"))),
    ));
    let listed = summary(&host, matter);
    assert_eq!(
        listed,
        MatterSummary {
            matter,
            name: "State v. Kunal Desai".to_owned(),
            kind: MatterKind::Folder,
            last_activity: Some(now),
            client: Some(client("Kunal Desai")),
            next_date: Some(date("2026-11-20")),
            pinned: false,
            last_opened: None,
            status: MatterStatus::Idle,
            folder_display: work.join("State v. Kunal Desai").display().to_string(),
            folder_state: FolderState::Found,
        }
    );
    // The Scratch matter is listed first (oldest), with no details, and outside this home it
    // would show in full; here it is below the home folder too.
    let all = host.matters();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, MatterKind::Scratch);
    assert_eq!(all[0].client, None);
    assert_eq!(all[1].matter, matter);
    host.shutdown();

    // A restart reads the same row and the same last activity back.
    let host = dir.start_with(1, |config| config.with_home_folder(Some(home)));
    assert_eq!(summary(&host, matter), listed);
    host.shutdown();
}

#[test]
fn a_folder_outside_home_shows_in_full() {
    let dir = Dir::new("folder_outside_home", &[]);
    let elsewhere = dir.data().join("home");
    let host = dir.start_with(0, |config| config.with_home_folder(Some(elsewhere)));
    let folder = dir.folder("Acme", &[]);
    let matter = created(import(&host, "Acme", &folder, (None, None)));
    assert_eq!(
        summary(&host, matter).folder_display,
        folder.display().to_string()
    );
    host.shutdown();
}

#[test]
fn a_default_location_create_makes_the_folder_there() {
    let dir = Dir::new("default_location_create", &[]);
    let location = dir.data().join("work").join("Matters");
    dir.set_settings(
        &[],
        serde_json::json!({ "matters": { "default_location": location.display().to_string() } }),
    );
    let host = dir.start_with(0, |config| {
        config.with_home_folder(Some(dir.data().to_owned()))
    });
    let create = |name: &str| HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source: MatterSource::DefaultLocation,
        client: Some(client("Rathi Steels")),
        next_date: None,
    };
    let matter = created(host.dispatch(create("Rathi Steels v. Northgate")));
    let folder = location.join("Rathi Steels v. Northgate");
    assert!(
        folder.join(".clauseo").join("matter.json").is_file(),
        "the folder was made"
    );
    let listed = summary(&host, matter);
    assert_eq!(listed.client, Some(client("Rathi Steels")));
    assert_eq!(
        listed.folder_display,
        Path::new("work")
            .join("Matters")
            .join("Rathi Steels v. Northgate")
            .display()
            .to_string()
    );
    // The same name again: the folder exists, so it is refused and nothing is made.
    let error = host.refused(
        create("Rathi Steels v. Northgate"),
        ErrorCode::CommandInvalid,
    );
    assert!(error.message().contains("already exists"), "{error}");
    assert_eq!(host.matters().len(), 2);
    // A name that is no folder name is refused by its shape.
    host.refused(create("Rathi: Steels"), ErrorCode::CommandInvalid);
    host.shutdown();
}

#[test]
fn without_a_set_location_the_default_is_documents_clauseo() {
    let dir = Dir::new("default_location_unset", &[]);
    let host = dir.start(0);
    let matter = created(host.dispatch(HostCommand::CreateMatter {
        name: Title::new("Kaveri Agro").unwrap(),
        source: MatterSource::DefaultLocation,
        client: None,
        next_date: None,
    }));
    let folder = dir
        .data()
        .join("Documents")
        .join("Clauseo")
        .join("Kaveri Agro");
    assert!(folder.is_dir(), "{}", folder.display());
    assert_eq!(summary(&host, matter).kind, MatterKind::Folder);
    host.shutdown();
}

#[test]
fn update_matter_keeps_clears_and_sets_each_detail() {
    let dir = Dir::new("update_matter", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[]);
    let matter = created(import(
        &host,
        "m",
        &folder,
        (Some(client("Acme")), Some(date("2026-12-01"))),
    ));
    let update = |client, next_date, pinned| HostCommand::UpdateMatter {
        matter,
        client,
        next_date,
        pinned,
    };
    // Nothing named: nothing changes, and the answer is still Done.
    assert_eq!(
        host.ok(update(FieldChange::Keep, FieldChange::Keep, None)),
        CommandOutcome::Done
    );
    let listed = summary(&host, matter);
    assert_eq!(
        (listed.client.clone(), listed.next_date, listed.pinned),
        (Some(client("Acme")), Some(date("2026-12-01")), false)
    );
    // Pin only.
    host.ok(update(FieldChange::Keep, FieldChange::Keep, Some(true)));
    let listed = summary(&host, matter);
    assert_eq!(
        (listed.client.clone(), listed.next_date, listed.pinned),
        (Some(client("Acme")), Some(date("2026-12-01")), true)
    );
    // Clear the client, set a new date.
    host.ok(update(
        FieldChange::Clear,
        FieldChange::Set(date("2027-01-04")),
        None,
    ));
    let listed = summary(&host, matter);
    assert_eq!(
        (listed.client.clone(), listed.next_date, listed.pinned),
        (None, Some(date("2027-01-04")), true)
    );
    // Set the client, clear the date, unpin.
    host.ok(update(
        FieldChange::Set(client("Beta LLP")),
        FieldChange::Clear,
        Some(false),
    ));
    let after = summary(&host, matter);
    assert_eq!(
        (after.client.clone(), after.next_date, after.pinned),
        (Some(client("Beta LLP")), None, false)
    );
    // An unknown matter.
    let unknown = MatterId::new(crate::support::foreign_uuid(9)).unwrap();
    host.refused(
        HostCommand::UpdateMatter {
            matter: unknown,
            client: FieldChange::Keep,
            next_date: FieldChange::Keep,
            pinned: Some(true),
        },
        ErrorCode::CommandUnknownMatter,
    );
    host.shutdown();
    let host = dir.start(1);
    assert_eq!(
        summary(&host, matter),
        after,
        "the details survive a restart"
    );
    host.shutdown();
}

#[test]
fn open_matter_records_last_opened() {
    let dir = Dir::new("open_matter", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[]);
    let matter = created(import(&host, "m", &folder, (None, None)));
    assert_eq!(summary(&host, matter).last_opened, None);
    host.clock.advance(Duration::from_secs(60));
    let opened = host.clock.now().wall;
    assert_eq!(
        host.ok(HostCommand::OpenMatter { matter }),
        CommandOutcome::Done
    );
    assert_eq!(summary(&host, matter).last_opened, Some(opened));
    let unknown = MatterId::new(crate::support::foreign_uuid(9)).unwrap();
    host.refused(
        HostCommand::OpenMatter { matter: unknown },
        ErrorCode::CommandUnknownMatter,
    );
    host.shutdown();
    let host = dir.start(1);
    assert_eq!(summary(&host, matter).last_opened, Some(opened));
    host.shutdown();
}

#[test]
fn last_activity_follows_the_newest_event_of_the_matter_and_its_threads() {
    let dir = Dir::new(
        "last_activity",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let made = summary(&host, matter).last_activity.unwrap();
    host.clock.advance(Duration::from_secs(90));
    let mut events = host.events(StreamId::Thread(thread));
    let accepted = host.ok(send_prompt(thread, "Summarise."));
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("{accepted:?}");
    };
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    let after = summary(&host, matter).last_activity.unwrap();
    assert!(after > made, "{after:?} after {made:?}");
    assert_eq!(after, host.clock.now().wall);
    host.shutdown();
    // After a restart it comes from each stream's last event in the store.
    let host = dir.start(1);
    assert_eq!(summary(&host, matter).last_activity, Some(after));
    host.shutdown();
}

#[cfg(unix)]
#[test]
fn a_home_reached_through_a_link_still_shortens_the_folder() {
    let dir = Dir::new("home_through_link", &[]);
    // The link stands for the work folder, where the matter folders are.
    let link = dir.data().join("home-link");
    crate::write::symlink(dir.work(), &link);
    let host = dir.start_with(0, |config| config.with_home_folder(Some(link.clone())));
    let folder = dir.folder("Acme", &[]);
    // Imported by its real path, as the room's folder picker gives it: on macOS the temp folder
    // is under `/var`, itself a link to `/private/var`, so the path as written matches neither
    // spelling of the home.
    let folder = dunce::canonicalize(&folder).unwrap();
    let matter = created(import(&host, "Acme", &folder, (None, None)));
    assert_eq!(summary(&host, matter).folder_display, "Acme");
    host.shutdown();
}

/// `GetDefaultLocation`'s display.
fn default_display(host: &Running) -> String {
    match host.query(HostQuery::GetDefaultLocation).unwrap() {
        clauseo_protocol::QueryResult::DefaultLocation(location) => location.display,
        other => panic!("GetDefaultLocation answered {other:?}"),
    }
}

#[test]
fn get_default_location_shows_where_new_matters_go() {
    let dir = Dir::new("get_default_location", &[]);
    // Not set: Documents/Clauseo, below the home folder.
    let host = dir.start_with(0, |config| {
        config.with_home_folder(Some(dir.data().to_owned()))
    });
    assert_eq!(
        default_display(&host),
        Path::new("Documents").join("Clauseo").display().to_string()
    );
    host.shutdown();
    // Set, below home; then the same with no home known, in full.
    let location = dir.data().join("work").join("Matters");
    dir.set_settings(
        &[],
        serde_json::json!({ "matters": { "default_location": location.display().to_string() } }),
    );
    let host = dir.start_with(1, |config| {
        config.with_home_folder(Some(dir.data().to_owned()))
    });
    assert_eq!(
        default_display(&host),
        Path::new("work").join("Matters").display().to_string()
    );
    // A default-location create lands where the display says.
    let matter = created(host.dispatch(HostCommand::CreateMatter {
        name: Title::new("Acme").unwrap(),
        source: MatterSource::DefaultLocation,
        client: None,
        next_date: None,
    }));
    assert_eq!(
        summary(&host, matter).folder_display,
        Path::new(&default_display(&host))
            .join("Acme")
            .display()
            .to_string()
    );
    host.shutdown();
    let host = dir.start_with(2, |config| config.with_home_folder(None));
    assert_eq!(default_display(&host), location.display().to_string());
    host.shutdown();
}
