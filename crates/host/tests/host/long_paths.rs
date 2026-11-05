//! A matter whose files run past 260 UTF-16 units: a folder of about 200 units with a
//! Devanagari part, and a file nested in it to past 300. A turn rewrites the file, the lawyer
//! undoes the step, and a second turn rewrites it again, each with its checkpoints. On Windows
//! every write and checkpoint here goes past `MAX_PATH`; on Linux and macOS the test only shows
//! the same flow passes. The folder stays under 248 units: Windows starts the agent with the
//! matter folder as its working folder, which `CreateProcessW` caps at `MAX_PATH` whatever
//! the prefix (std strips `\\?\` there), a limit outside this test.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    CommandOutcome, DomainEvent, FileChoice, FileDecision, HostCommand, MatterPath, StopReason,
    StreamId,
};

use crate::support::{Dir, FAKE, Scenario};

/// The file `long-paths.jsonl` writes, relative to the matter folder (119 UTF-16 units).
const FILE: &str = "दस्तावेज़-अनुबंध/second-level-folder-of-the-matter/third-level-folder-of-the-matter/\
                    fourth-level-folder/lease-draft.txt";

/// The length of `path` in UTF-16 units, as Windows counts it against `MAX_PATH`.
fn units(path: &Path) -> usize {
    path.to_string_lossy().encode_utf16().count()
}

/// `work` with a Devanagari folder and then plain ones until it is 190 units or more; each
/// adds at most 34, so it ends under 225.
fn deep_folder(work: &Path) -> PathBuf {
    let mut folder = work.join("मामले-अनुबंध");
    let mut n = 0;
    while units(&folder) < 190 {
        n += 1;
        folder.push(format!("client-folder-of-the-matter-{n:02}"));
    }
    folder
}

#[test]
fn a_file_past_300_units_is_written_undone_and_checkpointed() {
    let dir = Dir::new("long_paths", &[(FAKE, Scenario::Local("long-paths"))]);
    let host = dir.start(0);
    let rel = deep_folder(dir.work());
    let rel = rel
        .strip_prefix(dir.work())
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let folder = dir.folder(&rel, &[(FILE, "First draft\n")]);
    let file = folder.join(FILE);
    assert!((190..248).contains(&units(&folder)), "{}", units(&folder));
    assert!(units(&file) > 300, "{}", units(&file));

    let matter = host.create_matter("Deep", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let read = || std::fs::read_to_string(&file).unwrap();

    let (first, _) = host.prompt(thread, "Redraft the lease.");
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some(), "{}", events.dump());
    assert_eq!(read(), "Second draft\n");

    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn: first,
        files: vec![FileDecision {
            path: MatterPath::new(FILE).unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let restored = events.events().find_map(|event| match event {
        DomainEvent::StepUndone(undone) => Some(undone.restored.clone()),
        _ => None,
    });
    assert_eq!(restored, Some(vec![MatterPath::new(FILE).unwrap()]));
    assert_eq!(read(), "First draft\n");

    let (second, _) = host.prompt(thread, "Redraft it again.");
    let ended = events.until_turn_ended(second);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some(), "{}", events.dump());
    assert_eq!(read(), "Third draft\n");
}
