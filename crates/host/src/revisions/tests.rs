//! The pure parts of the tracked-changes job: stetdoc's answers read into revisions, the batch,
//! the dates, stetdoc's exits mapped to codes, and its environment.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_profile::skills::stetdoc::{Ran, RunFailure};
use clauseo_protocol::{
    ErrorCode, MAX_REVISION_TEXT_CHARS, MAX_REVISIONS, MatterPath, RevisionDecision, RevisionKind,
};
use serde_json::json;

use super::parse::{all_chunks, batch, is_docx, revisions, timestamp};
use super::stetdoc::{answer, env};

fn code(result: Result<serde_json::Value, crate::error::Error>) -> ErrorCode {
    result.unwrap_err().to_wire().code()
}

/// stetdoc's exit `code` with `stdout`, as `run_for` returns it.
fn exited(code: i32, stdout: &str) -> Ran {
    Ran {
        code: Some(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr_tail: String::new(),
    }
}

#[test]
fn check_and_view_join_into_revisions_in_document_order() {
    let check = json!({"existing": [
        {"author": "Other", "changes": [
            {"block": "p10", "date": "2026-11-09T16:43:50Z", "ids": [5], "kind": "insertion"},
            {"block": "p4", "ids": [3, 2], "kind": "paragraph_deleted"},
        ]},
        {"author": "Clauseo", "changes": [
            {"block": "p9", "date": "2026-11-09T16:43:50Z", "ids": [7], "kind": "format"},
            {"block": "p9", "date": "nonsense", "ids": [1], "kind": "move_from"},
        ]},
    ]});
    let ledger = concat!(
        "{stet 1 view=ledger}\n",
        "#2,3 delete O 2026-11-09 p4 \"Old \\\"para\\\".\"\n",
        "#5 insert O 2026-11-09 p10 \"नया\"\n",
        "#7 format C 2026-11-09 p9\n",
    );
    let view = json!({"text": ledger, "next": null});
    let (list, truncated, total) = revisions(&check, &view).unwrap();
    assert!(!truncated);
    assert_eq!(usize::try_from(total).unwrap(), list.len());
    let shape: Vec<_> = list
        .iter()
        .map(|r| (r.block.as_str(), r.ids.clone(), r.kind, r.text.as_deref()))
        .collect();
    assert_eq!(
        shape,
        [
            (
                "p4",
                vec![2, 3],
                RevisionKind::Delete,
                Some("Old \"para\".")
            ),
            ("p9", vec![1], RevisionKind::Move, None),
            ("p9", vec![7], RevisionKind::Format, None),
            ("p10", vec![5], RevisionKind::Insert, Some("नया")),
        ]
    );
    assert_eq!(list[0].author, "Other");
    assert_eq!(list[0].at, None);
    assert_eq!(list[1].at, None, "a date that does not read is left out");
    assert_eq!(list[2].author, "Clauseo");
    assert_eq!(list[3].at.unwrap().as_millis(), 1_794_242_630_000);
}

#[test]
fn a_long_list_is_cut_and_long_text_is_cut() {
    let long = "x".repeat(MAX_REVISION_TEXT_CHARS + 5);
    let changes: Vec<_> = (0..=u32::try_from(MAX_REVISIONS).unwrap())
        .map(|id| json!({"block": "p1", "ids": [id], "kind": "insertion"}))
        .collect();
    let check = json!({"existing": [{"author": long, "changes": changes}]});
    let view = json!({"text": format!("#0 insert X 2026-11-09 p1 \"{long}\"\n")});
    let (list, truncated, total) = revisions(&check, &view).unwrap();
    assert!(truncated);
    assert_eq!(usize::try_from(total).unwrap(), MAX_REVISIONS + 1);
    // Accept all names every one, past the cut, in chunks of at most the cut.
    let chunks = all_chunks(&check, MAX_REVISIONS).unwrap();
    let lens: Vec<usize> = chunks.iter().map(Vec::len).collect();
    assert_eq!(lens, [MAX_REVISIONS, 1]);
    assert_eq!(chunks[0].first(), Some(&0));
    assert_eq!(chunks[1], [u32::try_from(MAX_REVISIONS).unwrap()]);
    assert_eq!(list.len(), MAX_REVISIONS);
    assert_eq!(list[0].author.chars().count(), MAX_REVISION_TEXT_CHARS);
    assert_eq!(
        list[0].text.as_deref().unwrap().chars().count(),
        MAX_REVISION_TEXT_CHARS
    );
    // An answer of another shape is a failed edit.
    let bad = revisions(&json!({"existing": "no"}), &view).map(|_| json!(null));
    assert_eq!(code(bad), ErrorCode::WordEditFailed);
}

#[test]
fn a_change_s_ids_stay_in_one_chunk_and_an_id_is_named_once() {
    let check = json!({"existing": [
        {"author": "A", "changes": [
            {"block": "p1", "ids": [1], "kind": "insertion"},
            {"block": "p2", "ids": [3, 2], "kind": "deletion"},
            {"block": "p3", "ids": [4], "kind": "insertion"},
        ]},
        {"author": "B", "changes": [
            {"block": "p4", "ids": [5, 4], "kind": "deletion"},
        ]},
    ]});
    assert_eq!(
        all_chunks(&check, 2).unwrap(),
        [vec![1], vec![2, 3], vec![4, 5]]
    );
    assert_eq!(all_chunks(&check, 5).unwrap(), [vec![1, 2, 3, 4, 5]]);
    assert_eq!(
        all_chunks(&json!({"existing": []}), 5).unwrap(),
        Vec::<Vec<u32>>::new()
    );
}

#[test]
fn the_batch_dates_and_names() {
    assert_eq!(
        batch(RevisionDecision::Accept, &[1, 2]),
        "@accept\nids: 1, 2\n"
    );
    assert_eq!(batch(RevisionDecision::Reject, &[9]), "@reject\nids: 9\n");
    assert_eq!(
        timestamp("1970-01-01T00:00:01Z").unwrap().as_millis(),
        1_000
    );
    assert_eq!(
        timestamp("2024-02-29T23:59:59Z").unwrap().as_millis(),
        1_709_251_199_000
    );
    for bad in [
        "2026-11-09",
        "2026-13-09T00:00:00Z",
        "2026-11-09T24:00:00Z",
        "2026-11-09T16:43:50",
        "2026-11-09 16:43:50Z",
        "२०२६-11-09T16:43:50Z",
    ] {
        assert_eq!(timestamp(bad), None, "{bad}");
    }
    for (name, docx) in [
        ("a.docx", true),
        ("dir/B.DOCX", true),
        ("a.doc", false),
        ("docx", false),
        ("a.docx.txt", false),
    ] {
        assert_eq!(is_docx(&MatterPath::new(name).unwrap()), docx, "{name}");
    }
}

#[test]
fn stetdocs_exits_map_to_codes() {
    assert_eq!(answer(Ok(exited(0, "{\"a\":1}"))).unwrap(), json!({"a": 1}));
    assert_eq!(
        code(answer(Ok(exited(0, "not json")))),
        ErrorCode::WordEditFailed
    );
    let refusal = |name: &str| format!("{{\"error\":{{\"code\":\"{name}\",\"message\":\"m\"}}}}");
    for (name, exit, expected) in [
        ("not_found", 3, ErrorCode::WordRevisionsStale),
        ("stale_ref", 4, ErrorCode::WordRevisionsStale),
        ("stale_target", 4, ErrorCode::WordRevisionsStale),
        ("stale_files", 4, ErrorCode::WordRevisionsStale),
        ("malformed_input", 6, ErrorCode::WordNotDocx),
        ("corrupt_complement", 6, ErrorCode::WordNotDocx),
        ("invalid_payload", 2, ErrorCode::WordEditFailed),
        ("internal", 7, ErrorCode::WordEditFailed),
    ] {
        assert_eq!(
            code(answer(Ok(exited(exit, &refusal(name))))),
            expected,
            "{name}"
        );
    }
    assert_eq!(code(answer(Ok(exited(1, "")))), ErrorCode::WordEditFailed);
    let failed = |failure: RunFailure| code(answer(Err(failure)));
    assert_eq!(
        failed(RunFailure::Launch("gone".to_owned())),
        ErrorCode::WordStetdocMissing
    );
    assert_eq!(failed(RunFailure::TimedOut), ErrorCode::WordEditFailed);
    assert_eq!(
        failed(RunFailure::TooMuchOutput { max: 1 }),
        ErrorCode::WordEditFailed
    );
}

/// stetdoc's environment on top of a cleared one: nothing off Windows; on Windows the host's
/// `SystemRoot` (or the Windows directory when the host has none) and `TEMP` and `TMP` in the
/// job's folder, whatever the host's own say.
#[test]
fn stetdoc_gets_system_root_and_a_temp_folder_of_its_own_on_windows() {
    let work = Path::new("/data/revisions/m1");
    let host = |name: &str| {
        [
            ("SystemRoot", r"D:\Win"),
            ("TEMP", r"C:\Temp"),
            ("TMP", r"C:\Tmp"),
            ("PATH", "x"),
        ]
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| OsString::from(value))
    };
    let dir = || Some(OsString::from(r"E:\Windows"));
    assert!(env(false, host, dir, work).is_empty());
    let pairs = |env: BTreeMap<OsString, OsString>| -> Vec<(String, String)> {
        env.into_iter()
            .map(|(key, value)| (key.into_string().unwrap(), value.into_string().unwrap()))
            .collect()
    };
    let work_text = work.to_str().unwrap().to_owned();
    assert_eq!(
        pairs(env(true, host, dir, work)),
        [
            ("SystemRoot".to_owned(), r"D:\Win".to_owned()),
            ("TEMP".to_owned(), work_text.clone()),
            ("TMP".to_owned(), work_text.clone()),
        ]
    );
    // No SystemRoot in the host's environment: the Windows directory; neither: left out.
    let empty = |_: &str| None;
    assert_eq!(
        pairs(env(true, empty, dir, work))[0],
        ("SystemRoot".to_owned(), r"E:\Windows".to_owned())
    );
    assert_eq!(
        pairs(env(true, empty, || None, work)),
        [
            ("TEMP".to_owned(), work_text.clone()),
            ("TMP".to_owned(), work_text),
        ]
    );
}
