//! Tracked changes in a matter's Word file, with a fake stetdoc (`examples/stetdoc_fake.rs`):
//! `ListRevisions` reads a private copy, `DecideRevisions` writes the file between checkpoints
//! and `RestoreVersion` of its `before` undoes it; every refusal writes nothing, and the ones
//! decided before the edit never run stetdoc's `edit`. The real stetdoc is in
//! `revisions_real.rs`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::HostConfig;
use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DecideRevisions, DocVersion, ErrorCode, HostCommand, HostQuery,
    MAX_REVISION_TEXT_CHARS, MAX_REVISIONS, MatterId, MatterPath, QueryResult, RestoreRequest,
    RevisionDecision, RevisionKind, RevisionList, StreamId,
};
use sha2::{Digest as _, Sha256};

use crate::permissions::{PERM, until_first_request};
use crate::support::{Dir, Running, Scenario};
use crate::versions::matter_events;
use crate::write;

/// How long stetdoc's edit may take (the host's `EDIT_LIMIT`).
const EDIT_LIMIT: Duration = Duration::from_secs(30);

/// The Word file's bytes in these tests: the fake reads them as they are.
const ORIGINAL: &[u8] = b"PK fake docx bytes";

const CHECK: &str = r#"{"existing": [{"author": "Asha Rao", "changes": [
    {"block": "p2", "date": "2026-11-09T16:43:50Z", "ids": [1], "kind": "insertion"},
    {"block": "p1", "ids": [3, 2], "kind": "deletion"}
]}]}"#;

const VIEW: &str = r#"{"text": "{stet 1 view=ledger}\n#1 insert A 2026-11-09 p2 \"New clause.\"\n#2,3 delete A 2026-11-09 p1 \"Old clause.\"\n"}"#;

/// A host with the fake stetdoc (or none), and one matter holding `Lease.docx`.
struct Rig {
    dir: Dir,
    host: Running,
    matter: MatterId,
    folder: PathBuf,
    /// The fake's own folder: its scripts and its call log.
    bin: PathBuf,
}

impl Rig {
    fn new(test: &str, stetdoc: bool) -> Self {
        let dir = Dir::new(test, &[(PERM, Scenario::Bundled("permission-kinds"))]);
        let bin = dir.home().join("stetdoc");
        let program = fake_into(&bin);
        write::write(&bin.join("check.json"), CHECK.as_bytes());
        write::write(&bin.join("view.json"), VIEW.as_bytes());
        let host = dir.start_with(0, |config: HostConfig| {
            if stetdoc {
                config.with_stetdoc(program, "Clauseo".to_owned())
            } else {
                config
            }
        });
        let folder = dir.work_path("lease");
        write::write(&folder.join("Lease.docx"), ORIGINAL);
        let matter = host.create_matter("lease", &folder);
        Self {
            dir,
            host,
            matter,
            folder,
            bin,
        }
    }

    fn list(&self, path: &str) -> Result<RevisionList, clauseo_protocol::HostError> {
        let query = HostQuery::ListRevisions {
            matter: self.matter,
            path: lease(path),
        };
        self.host.query(query).map(|answer| match answer {
            QueryResult::Revisions(list) => list,
            other => panic!("not revisions: {other:?}"),
        })
    }

    fn decide(&self, version: DocVersion, ids: Vec<u32>) -> HostCommand {
        HostCommand::DecideRevisions(DecideRevisions {
            matter: self.matter,
            path: lease("Lease.docx"),
            version,
            ids,
            decision: RevisionDecision::Accept,
        })
    }

    fn lease_bytes(&self) -> Vec<u8> {
        std::fs::read(self.folder.join("Lease.docx")).unwrap()
    }

    /// Whether the fake's `edit` ran.
    fn edited(&self) -> bool {
        std::fs::read_to_string(self.bin.join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .any(|line| line == "edit")
    }

    /// Asserts nothing was written: the file as it was, stetdoc's copies gone.
    fn assert_untouched(&self) {
        assert_eq!(self.lease_bytes(), ORIGINAL);
        self.assert_no_copies();
    }

    fn assert_no_copies(&self) {
        let work = self
            .dir
            .data()
            .join("revisions")
            .join(self.matter.to_string());
        assert!(!work.exists(), "{} left behind", work.display());
    }
}

/// The fake stetdoc copied into `bin`, where it finds its scripts.
fn fake_into(bin: &Path) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let examples = exe.parent().unwrap().parent().unwrap().join("examples");
    let name = format!("stetdoc_fake{}", std::env::consts::EXE_SUFFIX);
    let program = bin.join(&name);
    write::write(&program, &std::fs::read(examples.join(&name)).unwrap());
    #[cfg(unix)]
    write::make_executable(&program);
    program
}

fn lease(path: &str) -> MatterPath {
    MatterPath::new(path).unwrap()
}

fn version_of(bytes: &[u8]) -> DocVersion {
    DocVersion::from_bytes(Sha256::digest(bytes).into())
}

#[test]
fn list_reads_a_private_copy_in_document_order() {
    let rig = Rig::new("revisions-list", true);
    let list = rig.list("Lease.docx").unwrap();
    assert_eq!(list.path, lease("Lease.docx"));
    assert_eq!(list.version, version_of(ORIGINAL));
    assert!(!list.truncated);
    let shape: Vec<_> = list
        .revisions
        .iter()
        .map(|r| (r.block.as_str(), r.ids.clone(), r.kind, r.text.as_deref()))
        .collect();
    assert_eq!(
        shape,
        [
            ("p1", vec![2, 3], RevisionKind::Delete, Some("Old clause.")),
            ("p2", vec![1], RevisionKind::Insert, Some("New clause.")),
        ]
    );
    assert_eq!(list.revisions[0].author, "Asha Rao");
    assert_eq!(
        list.revisions[1]
            .at
            .map(clauseo_protocol::Timestamp::as_millis),
        Some(1_794_242_630_000)
    );
    rig.assert_untouched();
    let mut names: Vec<_> = std::fs::read_dir(&rig.folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [".clauseo", "Lease.docx"],
        "stetdoc wrote in the matter"
    );
    rig.host.shutdown();
}

#[test]
fn accept_writes_the_file_and_a_restore_of_before_undoes_it() {
    let rig = Rig::new("revisions-accept-undo", true);
    let import = matter_events(&rig.host, rig.matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let outcome = rig.host.ok(rig.decide(version_of(ORIGINAL), vec![1]));
    let CommandOutcome::RevisionsDecided { before, checkpoint } = outcome else {
        panic!("not decided: {outcome:?}");
    };
    // The folder had not changed since the import: it holds the file as it was.
    assert_eq!(before, import);
    let mut accepted = ORIGINAL.to_vec();
    accepted.extend_from_slice(b"@accept\nids: 1\n");
    assert_eq!(rig.lease_bytes(), accepted);
    rig.assert_no_copies();
    rig.host.settle();
    let events = matter_events(&rig.host, rig.matter);
    assert_eq!(events.checkpoint(CheckpointKind::Revisions), checkpoint);
    assert!(checkpoint.is_some());

    let undo = HostCommand::RestoreVersion {
        matter: rig.matter,
        checkpoint: before,
        scope: RestoreRequest::Files {
            paths: vec![lease("Lease.docx")],
        },
    };
    assert!(matches!(
        rig.host.ok(undo),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(rig.lease_bytes(), ORIGINAL);
    rig.host.shutdown();
}

/// A failure after the rename (the `revisions` checkpoint's walk failing as it does on Windows
/// when it meets a file another program holds) answers the decision, not an error: the file is
/// written, `before` is there and its restore undoes it; no `checkpoint`, and none logged.
#[test]
fn a_failure_after_the_write_still_answers_decided_and_the_undo_works() {
    let rig = Rig::new("revisions-after-write", true);
    let import = matter_events(&rig.host, rig.matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    // The matter may hold its folder by another spelling of the same path (`/private/var`).
    let file = rig.folder.join("Lease.docx");
    clauseo_matters::test_support::fail_after_replace_write(&file);
    clauseo_matters::test_support::fail_after_replace_write(dunce::canonicalize(&file).unwrap());
    let outcome = rig.host.ok(rig.decide(version_of(ORIGINAL), vec![1]));
    let CommandOutcome::RevisionsDecided { before, checkpoint } = outcome else {
        panic!("not decided: {outcome:?}");
    };
    assert_eq!(before, import);
    assert_eq!(checkpoint, None);
    let mut accepted = ORIGINAL.to_vec();
    accepted.extend_from_slice(b"@accept\nids: 1\n");
    assert_eq!(rig.lease_bytes(), accepted);
    rig.assert_no_copies();
    rig.host.settle();
    assert_eq!(
        matter_events(&rig.host, rig.matter).checkpoint(CheckpointKind::Revisions),
        None
    );

    let undo = HostCommand::RestoreVersion {
        matter: rig.matter,
        checkpoint: before,
        scope: RestoreRequest::Files {
            paths: vec![lease("Lease.docx")],
        },
    };
    assert!(matches!(
        rig.host.ok(undo),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(rig.lease_bytes(), ORIGINAL);
    rig.host.shutdown();
}

/// A heavily redlined document's `view` runs past the skill runs' 1 MiB of stdout: the list
/// still reads, cut to the most revisions and the longest text the list carries.
#[test]
fn a_list_past_one_mib_reads_cut_to_its_caps() {
    let rig = Rig::new("revisions-big-list", true);
    let count = u32::try_from(MAX_REVISIONS).unwrap() + 100;
    let long = "x".repeat(MAX_REVISION_TEXT_CHARS + 1_000);
    let changes: Vec<_> = (1..=count)
        .map(|id| serde_json::json!({"block": format!("p{id}"), "ids": [id], "kind": "insertion"}))
        .collect();
    let check = serde_json::json!({"existing": [{"author": "Asha Rao", "changes": changes}]});
    let ledger = (1..=count)
        .map(|id| format!("#{id} insert A 2026-11-09 p{id} \"{long}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let view = serde_json::json!({ "text": ledger }).to_string();
    assert!(view.len() > 1024 * 1024, "{}", view.len());
    write::write(&rig.bin.join("check.json"), check.to_string().as_bytes());
    write::write(&rig.bin.join("view.json"), view.as_bytes());
    let list = rig.list("Lease.docx").unwrap();
    assert!(list.truncated);
    assert_eq!(list.revisions.len(), MAX_REVISIONS);
    assert_eq!(list.revisions[0].block, "p1");
    assert_eq!(
        list.revisions[0].text.as_deref().unwrap().chars().count(),
        MAX_REVISION_TEXT_CHARS
    );
    rig.host.shutdown();
}

/// Private copies a crash left under `<data>/revisions` are gone once the host has started.
#[test]
fn copies_a_crash_left_are_swept_at_start() {
    let dir = Dir::new(
        "revisions-sweep",
        &[(PERM, Scenario::Bundled("permission-kinds"))],
    );
    let left = dir
        .data()
        .join("revisions")
        .join("some-matter")
        .join("read.docx");
    write::write(&left, ORIGINAL);
    let host = dir.start(0);
    assert!(
        !dir.data().join("revisions").exists(),
        "{} left behind",
        left.display()
    );
    host.shutdown();
}

/// An attempt that fails after its outside-edits checkpoint (here: stetdoc's output is over the
/// size cap, so it cannot be stored) drops that checkpoint's log commit; the retry reuses it as
/// `before`, so the decision records it late and the room's undo, a restore of `before`, works.
#[test]
fn a_retry_after_a_failed_write_records_its_before_and_the_undo_restores_it() {
    let rig = Rig::new("revisions-retry-before", true);
    write::write(&rig.folder.join("notes.txt"), b"an edit outside Clauseo");
    write::write(&rig.bin.join("edit.txt"), b"huge");
    rig.host.refused(
        rig.decide(version_of(ORIGINAL), vec![1]),
        ErrorCode::MatterCheckpointFailed,
    );
    rig.assert_untouched();

    write::write(&rig.bin.join("edit.txt"), b"ok");
    let outcome = rig.host.ok(rig.decide(version_of(ORIGINAL), vec![1]));
    let CommandOutcome::RevisionsDecided { before, .. } = outcome else {
        panic!("not decided: {outcome:?}");
    };
    rig.host.settle();
    assert_eq!(
        matter_events(&rig.host, rig.matter).checkpoint(CheckpointKind::OutsideEdits),
        Some(before),
        "the reused before is recorded"
    );
    assert_ne!(rig.lease_bytes(), ORIGINAL);
    let undo = HostCommand::RestoreVersion {
        matter: rig.matter,
        checkpoint: before,
        scope: RestoreRequest::Files {
            paths: vec![lease("Lease.docx")],
        },
    };
    assert!(matches!(
        rig.host.ok(undo),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(rig.lease_bytes(), ORIGINAL);
    rig.host.shutdown();
}

/// A path that is a folder is not a Word file.
#[test]
fn a_folder_is_not_docx() {
    let rig = Rig::new("revisions-folder", true);
    write::write(&rig.folder.join("Bundle.docx").join("inner.txt"), b"x");
    let error = rig.list("Bundle.docx").unwrap_err();
    assert_eq!(error.code(), ErrorCode::WordNotDocx);
    rig.host.shutdown();
}

#[test]
fn a_file_open_in_word_is_refused_before_stetdoc_runs() {
    let rig = Rig::new("revisions-locked", true);
    write::write(
        &rig.folder.join("~$Lease.docx"),
        b"owner file: open in Word",
    );
    let error = rig.host.refused(
        rig.decide(version_of(ORIGINAL), vec![1]),
        ErrorCode::MatterFileLocked,
    );
    assert_eq!(error.files(), [lease("Lease.docx")]);
    assert!(!error.message().contains("Lease"), "{}", error.message());
    assert!(!rig.edited(), "stetdoc ran for a locked file");
    rig.assert_untouched();
    rig.host.shutdown();
}

#[test]
fn a_stale_version_is_refused_before_stetdoc_runs() {
    let rig = Rig::new("revisions-stale", true);
    rig.host.refused(
        rig.decide(version_of(b"what the list read"), vec![1]),
        ErrorCode::WordRevisionsStale,
    );
    assert!(!rig.edited(), "stetdoc ran for a stale list");
    rig.assert_untouched();
    rig.host.shutdown();
}

#[test]
fn no_stetdoc_and_a_file_that_is_not_docx_are_refused() {
    let rig = Rig::new("revisions-missing", false);
    let error = rig.list("Lease.docx").unwrap_err();
    assert_eq!(error.code(), ErrorCode::WordStetdocMissing);
    rig.host.refused(
        rig.decide(version_of(ORIGINAL), vec![1]),
        ErrorCode::WordStetdocMissing,
    );
    let error = rig.list("Lease.pdf").unwrap_err();
    assert_eq!(error.code(), ErrorCode::WordNotDocx);
    rig.assert_untouched();
    rig.host.shutdown();
}

#[test]
fn stetdoc_failing_or_answering_badly_is_a_failed_edit_and_writes_nothing() {
    let rig = Rig::new("revisions-failed", true);
    for (script, code) in [
        ("internal 7", ErrorCode::WordEditFailed),
        ("garbage", ErrorCode::WordEditFailed),
        ("not_found 3", ErrorCode::WordRevisionsStale),
        ("malformed_input 6", ErrorCode::WordNotDocx),
    ] {
        write::write(&rig.bin.join("edit.txt"), script.as_bytes());
        rig.host
            .refused(rig.decide(version_of(ORIGINAL), vec![1]), code);
        rig.assert_untouched();
    }
    // The writer was given back each time: a decision that works still goes through.
    write::write(&rig.bin.join("edit.txt"), b"ok");
    rig.host.ok(rig.decide(version_of(ORIGINAL), vec![2, 3]));
    rig.host.shutdown();
}

#[test]
fn stetdoc_past_its_deadline_is_killed_and_writes_nothing() {
    let rig = Rig::new("revisions-timeout", true);
    write::write(&rig.bin.join("edit.txt"), b"hang");
    let command = rig.decide(version_of(ORIGINAL), vec![1]);
    // Only the timers armed from here on: the edit's deadline is the first of its length.
    rig.host.clock.forget_armed();
    let answer = std::thread::scope(|scope| {
        let sent = scope.spawn(|| rig.host.dispatch(command));
        rig.host.wait_armed(EDIT_LIMIT);
        rig.host.clock.advance(EDIT_LIMIT);
        sent.join().unwrap()
    });
    assert_eq!(answer.unwrap_err().code(), ErrorCode::WordEditFailed);
    rig.assert_untouched();
    rig.host.shutdown();
}

#[test]
fn a_decision_waits_for_a_running_turn() {
    let rig = Rig::new("revisions-turn-running", true);
    let thread = rig.host.create_thread(rig.matter, PERM);
    let mut events = rig.host.events(StreamId::Thread(thread));
    rig.host.prompt(thread, "Go.");
    until_first_request(&mut events);
    let error = rig.host.refused(
        rig.decide(version_of(ORIGINAL), vec![1]),
        ErrorCode::MatterTurnRunning,
    );
    assert!(error.to_string().contains("a turn is running"), "{error}");
    assert!(!rig.edited());
    rig.assert_untouched();
    rig.host.shutdown();
}
