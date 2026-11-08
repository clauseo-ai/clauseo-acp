//! The access-denied match and what a tool call names.

use clauseo_protocol::{
    ContentBlock, MatterPath, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus,
    ToolLocation,
};

use super::{Seen, says_access_denied};

#[test]
fn access_denied_in_each_os_wording() {
    for text in [
        "Access is denied. (os error 5)",
        "ERROR_ACCESS_DENIED",
        "Error: EACCES: permission denied, open '/Users/a/x'",
        "cat: /etc/shadow: Permission denied",
        "rm: x: Operation not permitted",
        "EPERM",
        "failed (eperm)",
    ] {
        assert!(says_access_denied(text), "{text}");
    }
    for text in [
        "No such file or directory",
        "ENOENT: no such file",
        "the permit was denied by the board",
        "EPERMANENT",
        "keeperman",
    ] {
        assert!(!says_access_denied(text), "{text}");
    }
}

fn failed_with(text: &str) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        content: Some(vec![ToolCallContent::Text {
            text: TextOrBlob::Inline(text.into()),
        }]),
        ..ToolCallPatch::default()
    }
}

#[test]
fn only_a_failed_call_with_the_message_counts() {
    assert!(Seen::patch(&failed_with("Access is denied.")).failed_access_denied());
    assert!(!Seen::patch(&failed_with("file not found")).failed_access_denied());
    let mut completed = failed_with("Access is denied.");
    completed.status = Some(ToolCallStatus::Completed);
    assert!(!Seen::patch(&completed).failed_access_denied());
    // The message in the raw output, or in a text block, counts too.
    let output = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        raw_output: Some(TextOrBlob::Inline(r#"{"error":"EACCES"}"#.into())),
        ..ToolCallPatch::default()
    };
    assert!(Seen::patch(&output).failed_access_denied());
    let block = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        content: Some(vec![ToolCallContent::Content {
            content: ContentBlock::Text {
                text: "Permission denied".into(),
            },
        }]),
        ..ToolCallPatch::default()
    };
    assert!(Seen::patch(&block).failed_access_denied());
}

#[test]
fn paths_come_from_locations_then_the_input() {
    let patch = ToolCallPatch {
        locations: Some(vec![
            ToolLocation::Inside {
                path: MatterPath::new("a.docx").unwrap(),
                line: None,
            },
            ToolLocation::Outside {
                display: "/home/a/notes.docx".into(),
            },
        ]),
        raw_input: Some(TextOrBlob::Inline(r#"{"file_path":"/srv/x"}"#.into())),
        ..ToolCallPatch::default()
    };
    assert_eq!(
        Seen::patch(&patch).paths(),
        vec!["a.docx", "/home/a/notes.docx", "/srv/x"]
    );
    let input = ToolCallPatch {
        raw_input: Some(TextOrBlob::Inline(
            r#"{"path":"C:\\Users\\a\\x.txt"}"#.into(),
        )),
        ..ToolCallPatch::default()
    };
    assert_eq!(Seen::patch(&input).paths(), vec![r"C:\Users\a\x.txt"]);
    let not_json = ToolCallPatch {
        raw_input: Some(TextOrBlob::Inline("ls -la".into())),
        ..ToolCallPatch::default()
    };
    assert_eq!(Seen::patch(&not_json).paths(), Vec::<String>::new());
}
