//! Reading a failed tool call as a refusal: the OS's and Claude Code's words, codes as whole
//! words only, and the paths a call names.

use std::path::PathBuf;

use clauseo_protocol::{ContentBlock, TextOrBlob, ToolCallContent, ToolLocation};

use super::{names_fenced, paths, says, says_refused};
use crate::sent::CallFields;

#[test]
fn the_refusal_words_are_recognised_and_codes_only_as_words() {
    for text in [
        "Permission to read C:\\M\\Smith\\lease.pdf has been denied.",
        "Permission to use Bash with command type x has been denied.",
        "<tool_use_error>File is in a directory that is denied by your permission settings.\
            </tool_use_error>",
        "File is covered by a Read deny rule in your permission settings and cannot be edited.",
        "File is covered by a Read deny rule in your permission settings and cannot be written.",
        "cat: /m/smith/a: Operation not permitted",
        "Get-Content : Access is denied",
        "EACCES: open failed",
        "error: EPERM",
    ] {
        assert!(says(text), "{text}");
    }
    for text in [
        "File not found",
        "no such file",
        "keepermission",
        "EACCESSIBLE",
    ] {
        assert!(!says(text), "{text}");
    }
}

#[test]
fn a_failed_calls_text_or_output_is_read_and_a_blob_is_not() {
    let text = |words: &str| ToolCallContent::Content {
        content: ContentBlock::Text {
            text: words.to_owned(),
        },
    };
    assert!(says_refused(
        &[text("Permission to read x has been denied.")],
        None
    ));
    let output = TextOrBlob::Inline("Access is denied.".to_owned());
    assert!(says_refused(&[], Some(&output)));
    assert!(!says_refused(&[text("Done.")], None));
}

#[test]
fn a_calls_paths_are_its_locations_and_its_inputs_taken_from_the_matter() {
    let own = PathBuf::from(crate::testing::abs("/m/own"));
    let outside = crate::testing::abs("/m/smith/lease.pdf");
    let locations = [
        ToolLocation::Inside {
            path: clauseo_protocol::MatterPath::new("child/a.txt").unwrap(),
            line: None,
        },
        ToolLocation::Outside {
            display: outside.clone(),
        },
    ];
    let input = TextOrBlob::Inline(r#"{"file_path": "notes/b.txt"}"#.to_owned());
    let fields = CallFields {
        kind: None,
        status: None,
        locations: &locations,
        content: &[],
        raw_input: Some(&input),
        tool_name: None,
    };
    let found = paths(&fields, &own);
    assert!(found.contains(&own.join("child/a.txt")), "{found:?}");
    assert!(found.contains(&PathBuf::from(&outside)), "{found:?}");
    assert!(found.contains(&own.join("notes/b.txt")), "{found:?}");
}

/// A failed call's text naming a folder of a matter the conversation is not invited into names
/// that matter, the drive in any case; a longer name, an invited matter or no path, none.
#[test]
fn a_fenced_folder_in_a_failed_calls_text_names_its_matter() {
    use crate::agent::matter_fence::{MatterFence, Other};

    let id = |n: u128| {
        let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
        clauseo_protocol::MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
    };
    let other = |n, folder: &str, invited| {
        let mut other = Other::new(id(n), format!("M{n}"), PathBuf::from(folder), invited);
        other.spellings = vec![PathBuf::from(folder)];
        other
    };
    let fence = MatterFence::asked(vec![
        other(1, r"C:\Users\clauseo-lawyer\Matters\B", false),
        other(2, r"C:\Users\clauseo-lawyer\Matters\Shared", true),
    ]);
    let said = |words: &str| {
        let text = ToolCallContent::Content {
            content: ContentBlock::Text {
                text: words.to_owned(),
            },
        };
        names_fenced(&fence, &[text], None)
    };
    assert_eq!(
        said(r"Get-Content : c:\users\clauseo-lawyer\Matters\B\lease.pdf: denied"),
        Some(id(1))
    );
    assert_eq!(
        said(r"no access to C:\Users\clauseo-lawyer\Matters\B."),
        Some(id(1))
    );
    assert_eq!(said(r"C:\Users\clauseo-lawyer\Matters\Bob\x"), None);
    assert_eq!(said(r"C:\Users\clauseo-lawyer\Matters\B.old\x"), None);
    assert_eq!(said(r"C:\Users\clauseo-lawyer\Matters\Shared\x"), None);
    assert_eq!(said("File does not exist."), None);
}
