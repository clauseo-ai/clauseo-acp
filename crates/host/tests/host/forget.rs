//! `ForgetMatter` at the interface: the matter leaves the list and nothing is deleted, an import
//! of its folder brings it back (after a restart, and moved in the same run), what it refuses, and
//! how a forgotten matter's folder counts for other folders.

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, ErrorCode, FieldChange, HostCommand, HostQuery, MatterKind,
    MatterSource, QueryResult, RestoreRequest, StopReason, StreamId, Title,
};

use crate::matters::{client, created, date, import, summary};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, send_prompt, wait};

/// The durable batch: armed by a message's first un-durable delta, so seeing it armed means the
/// agent's first chunk reached the host, as the cancel tests wait for it. (The display shows that
/// chunk at once, so no display timer is armed for it.)
const DURABLE: Duration = Duration::from_millis(250);

/// The cancel fallback's wait.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// Every file under `folder` with its bytes, sorted by path.
fn listing(folder: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![folder.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn forget_takes_the_matter_off_the_list_and_an_import_brings_it_back() {
    let dir = Dir::new("forget_matter", &[(FAKE, Scenario::Local("replay-seed"))]);
    let host = dir.start(0);
    let (matter, thread, folder) = matter_with_thread(
        &dir,
        &host,
        "Smith v Jones",
        &[("brief.txt", "Clause 4 draft\n")],
        FAKE,
    );
    host.ok(HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Set(client("Smith")),
        next_date: FieldChange::Keep,
        pinned: Some(true),
    });
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Tighten clause 4");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    let before = listing(&folder);

    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter }),
        CommandOutcome::Done
    );
    assert!(
        host.matters()
            .iter()
            .all(|summary| summary.matter != matter)
    );
    // Nothing in the folder changed: its papers and its marker are where they were.
    assert_eq!(listing(&folder), before);
    assert!(folder.join(".clauseo").join("matter.json").is_file());
    // The matter and its threads answer no command or query.
    host.refused(
        HostCommand::OpenMatter { matter },
        ErrorCode::CommandUnknownMatter,
    );
    host.refused(
        HostCommand::ForgetMatter { matter },
        ErrorCode::CommandUnknownMatter,
    );
    host.refused(
        send_prompt(thread, "Again."),
        ErrorCode::CommandUnknownThread,
    );
    host.refused(
        HostCommand::CreateThread {
            matter,
            agent: clauseo_protocol::AgentId::new(FAKE).unwrap(),
        },
        ErrorCode::CommandUnknownMatter,
    );
    let threads = host.query(HostQuery::ListThreads { matter }).unwrap_err();
    assert_eq!(threads.code(), ErrorCode::CommandUnknownMatter);
    host.shutdown();

    // Still forgotten after a restart; importing the folder again brings it back, history and
    // details included, under its own id. The agent now resumes the thread's session.
    dir.set_agents(&[(FAKE, Scenario::Local("replay-load"))]);
    let host = dir.start(1);
    assert!(
        host.matters()
            .iter()
            .all(|summary| summary.matter != matter)
    );
    let back = created(import(
        &host,
        "Another name",
        &folder,
        (None, Some(date("2027-02-02"))),
    ));
    assert_eq!(back, matter, "the same matter comes back");
    let listed = summary(&host, matter);
    assert_eq!(listed.name, "Smith v Jones", "it keeps its name");
    assert_eq!(listed.client, Some(client("Smith")), "and its details");
    assert!(listed.pinned);
    assert_eq!(
        listed.next_date,
        Some(date("2027-02-02")),
        "a detail the import gives wins"
    );
    let threads = host.threads(matter);
    assert_eq!(
        threads.iter().map(|t| t.thread).collect::<Vec<_>>(),
        vec![thread]
    );
    // Its thread works again.
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "And clause 5?");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}

#[test]
fn forget_is_refused_for_the_scratch_matter_and_while_a_turn_runs() {
    let dir = Dir::new(
        "forget_refused",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let host = dir.start(0);
    let scratch = host
        .matters()
        .into_iter()
        .find(|m| m.kind == MatterKind::Scratch)
        .unwrap();
    host.refused(
        HostCommand::ForgetMatter {
            matter: scratch.matter,
        },
        ErrorCode::CommandInvalid,
    );
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);
    let error = host.refused(
        HostCommand::ForgetMatter { matter },
        ErrorCode::MatterTurnRunning,
    );
    assert!(error.retryable());
    assert!(
        host.matters().iter().any(|m| m.matter == matter),
        "still listed"
    );
    host.ok(HostCommand::Cancel { thread });
    host.wait_armed(CANCEL_WAIT);
    host.clock.advance(CANCEL_WAIT);
    events.until_turn_ended(turn);
    host.settle();
    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter }),
        CommandOutcome::Done
    );
    host.shutdown();
}

/// `PreviewRestore` of the whole matter at its import checkpoint: it reads the folder on the
/// matter's worker.
fn preview(
    host: &crate::support::Running,
    matter: clauseo_protocol::MatterId,
) -> Result<QueryResult, clauseo_protocol::HostError> {
    let mut events = host.events(StreamId::Matter(matter));
    events.until("the import checkpoint", |e| {
        e.checkpoint(CheckpointKind::Import).is_some()
    });
    let checkpoint = events.checkpoint(CheckpointKind::Import).unwrap();
    host.query(HostQuery::PreviewRestore {
        matter,
        checkpoint,
        scope: RestoreRequest::Matter,
    })
}

#[test]
fn a_folder_moved_while_forgotten_comes_back_where_it_is_now_in_the_same_run() {
    let dir = Dir::new("forget_moved_same_run", &[]);
    let folder = dir.folder("Kaveri Agro", &[("brief.txt", "Clause 4\n")]);
    let host = dir.start(0);
    let matter = created(import(&host, "Kaveri Agro", &folder, (None, None)));
    // The matter's worker opens on the folder where it is.
    preview(&host, matter).unwrap();
    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter }),
        CommandOutcome::Done
    );
    let moved = dir.data().join("work").join("Arbitrations");
    crate::write::create_dir_all(&moved);
    let moved = moved.join("Kaveri Agro");
    crate::write::rename(&folder, &moved);
    assert_eq!(
        created(import(&host, "Kaveri Agro", &moved, (None, None))),
        matter
    );
    // Its versions read from the folder where it is now, not where the old worker had it.
    let answer = preview(&host, matter).unwrap();
    assert!(
        matches!(answer, QueryResult::RestorePreview(_)),
        "{answer:?}"
    );
    assert_eq!(
        summary(&host, matter).folder_display,
        moved.display().to_string()
    );
    host.shutdown();
}

#[test]
fn a_forgotten_folder_still_counts_for_other_folders() {
    let dir = Dir::new("forget_nesting", &[]);
    let folder = dir.folder("Smith v Jones", &[("a.txt", "x\n")]);
    let inside = dir.folder("Smith v Jones/Pleadings", &[("b.txt", "y\n")]);
    let host = dir.start(0);
    let matter = created(import(&host, "Smith v Jones", &folder, (None, None)));
    host.ok(HostCommand::ForgetMatter { matter });
    // A folder inside it or holding it is refused, naming no matter: the forgotten one is not
    // on the list, so there is nothing to open.
    for path in [inside, folder.parent().unwrap().to_owned()] {
        let error = import(&host, "Other", &path, (None, None)).expect_err("refused");
        assert_eq!(
            error.code(),
            ErrorCode::MatterFolderIsMatter,
            "{}: {error}",
            path.display()
        );
        assert_eq!(error.matter(), None, "{}", path.display());
    }
    let parent = wait(
        "register_folder",
        host.handle.register_folder(folder.clone()),
    )
    .unwrap();
    let error = host.refused(
        HostCommand::CreateMatter {
            name: Title::new("Appeal").unwrap(),
            source: MatterSource::NewFolder { folder: parent },
            client: None,
            next_date: None,
        },
        ErrorCode::MatterFolderIsMatter,
    );
    assert_eq!(error.matter(), None);
    assert!(!folder.join("Appeal").exists());
    // The folder itself still brings the matter back.
    assert_eq!(
        created(import(&host, "Smith v Jones", &folder, (None, None))),
        matter
    );
    assert!(
        host.matters()
            .iter()
            .any(|m| m.matter == matter && m.kind == MatterKind::Folder)
    );
    host.shutdown();
}
