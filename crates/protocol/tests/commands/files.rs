//! File lists of `UndoStep` and `RestoreVersion`: an undo names an `Undo` entry, no path is named
//! twice, and the count and byte caps hold. Shared helpers are in `commands.rs`.

use clauseo_protocol::{
    FileChoice, FileDecision, HostCommand, MAX_FILE_LIST, MAX_PATH_LIST_BYTES, MatterPath,
    RestoreRequest, RestoreScope,
};

use super::fixtures::{checkpoint_id, matter_id, path, thread_id, turn_id};
use super::{Validate, invalid, too_large};

#[test]
fn undo_step_needs_a_file_it_undoes() {
    let decision = |p: &str, choice| FileDecision {
        path: path(p),
        choice,
    };
    let undo = |list: Vec<FileDecision>| HostCommand::UndoStep {
        thread: thread_id(1),
        turn: turn_id(2),
        files: list,
    };
    let empty = format!("{:?}", undo(Vec::new()).validate());
    assert!(invalid(&undo(Vec::new()).validate()));
    // Keep-only lists undo nothing, so they are refused with the same error as the empty list.
    let keep_only = [
        vec![decision("a.docx", FileChoice::Keep)],
        vec![
            decision("a.docx", FileChoice::Keep),
            decision("b.docx", FileChoice::Keep),
        ],
    ];
    for list in keep_only {
        let result = undo(list).validate();
        assert!(invalid(&result), "{result:?}");
        assert_eq!(format!("{result:?}"), empty);
    }
    // One `Undo` among `Keep`s is enough.
    assert!(
        undo(vec![
            decision("a.docx", FileChoice::Keep),
            decision("b.docx", FileChoice::Undo),
        ])
        .validate()
        .is_ok()
    );
}

#[test]
fn file_list_rules() {
    let decision = |p: &str| FileDecision {
        path: path(p),
        choice: FileChoice::Undo,
    };
    let undo = |list: Vec<FileDecision>| HostCommand::UndoStep {
        thread: thread_id(1),
        turn: turn_id(2),
        files: list,
    };
    assert!(
        undo(vec![decision("a.docx"), decision("b.docx")])
            .validate()
            .is_ok()
    );
    assert!(invalid(
        &undo(vec![decision("a.docx"), decision("a.docx")]).validate()
    ));
    // An empty undo undoes nothing.
    assert!(invalid(&undo(Vec::new()).validate()));
    let restore = |scope| HostCommand::RestoreVersion {
        matter: matter_id(5),
        checkpoint: checkpoint_id(6),
        scope,
    };
    assert!(restore(RestoreRequest::Matter).validate().is_ok());
    assert!(
        restore(RestoreRequest::Files {
            paths: vec![path("a.docx")],
        })
        .validate()
        .is_ok()
    );
    assert!(invalid(
        &restore(RestoreRequest::Files { paths: Vec::new() }).validate()
    ));
    assert!(invalid(
        &restore(RestoreRequest::Files {
            paths: vec![path("a.docx"), path("a.docx")],
        })
        .validate()
    ));
    // The host logs the scope a command asked for as a `RestoreScope`.
    let files = vec![path("a.docx")];
    let request = RestoreRequest::Files {
        paths: files.clone(),
    };
    assert_eq!(
        RestoreScope::from(request),
        RestoreScope::Files { paths: files }
    );
    assert_eq!(
        RestoreScope::from(RestoreRequest::Matter),
        RestoreScope::Matter
    );
}

/// `count` distinct paths of `len` bytes each (`len` at least 6): a 5-digit number, then
/// `a`s in segments of at most 200 bytes.
fn long_paths(count: usize, len: usize) -> Vec<MatterPath> {
    (0..count)
        .map(|n| {
            let mut text = format!("{n:05}");
            while text.len() < len {
                let at_segment_end = text.len() % 201 == 200;
                text.push(if at_segment_end && text.len() + 1 < len {
                    '/'
                } else {
                    'a'
                });
            }
            path(&text)
        })
        .collect()
}

#[test]
fn file_lists_cap_their_path_bytes() {
    // 1024 paths of 1024 bytes are exactly 1 MiB; one byte more is over.
    let at_cap = long_paths(1024, 1024);
    let mut over = at_cap.clone();
    if let Some(last) = over.last_mut() {
        *last = path(&format!("{}b", last.as_str()));
    }
    let total = |list: &[MatterPath]| list.iter().map(|p| p.as_str().len()).sum::<usize>();
    assert_eq!(total(&at_cap), MAX_PATH_LIST_BYTES);
    assert_eq!(total(&over), MAX_PATH_LIST_BYTES + 1);
    // The first file is undone; the rest are kept, and count toward the cap all the same.
    let undo = |list: &[MatterPath]| HostCommand::UndoStep {
        thread: thread_id(1),
        turn: turn_id(2),
        files: list
            .iter()
            .enumerate()
            .map(|(n, p)| FileDecision {
                path: p.clone(),
                choice: if n == 0 {
                    FileChoice::Undo
                } else {
                    FileChoice::Keep
                },
            })
            .collect(),
    };
    let restore = |list: &[MatterPath]| HostCommand::RestoreVersion {
        matter: matter_id(5),
        checkpoint: checkpoint_id(6),
        scope: RestoreRequest::Files {
            paths: list.to_vec(),
        },
    };
    let builders: [fn(&[MatterPath]) -> HostCommand; 2] = [undo, restore];
    for build in builders {
        assert!(build(&at_cap).validate().is_ok());
        // Exactly MAX_FILE_LIST short paths pass the count cap; one more is refused (below).
        let at_count = long_paths(MAX_FILE_LIST, 6);
        assert!(build(&at_count).validate().is_ok(), "at the count cap");
        let result = build(&over).validate();
        assert!(
            too_large(&result, MAX_PATH_LIST_BYTES + 1, MAX_PATH_LIST_BYTES),
            "{result:?}"
        );
        // The count cap still holds for short paths.
        let many = long_paths(MAX_FILE_LIST + 1, 6);
        let result = build(&many).validate();
        assert!(
            too_large(&result, MAX_FILE_LIST + 1, MAX_FILE_LIST),
            "{result:?}"
        );
    }
}
