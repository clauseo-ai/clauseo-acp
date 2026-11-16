//! Reading a failed tool call as a refusal: the OS's and Claude Code's words, codes as whole
//! words only, and the paths a call names.

use std::path::PathBuf;

use clauseo_protocol::{ContentBlock, TextOrBlob, ToolCallContent, ToolLocation};

use super::{matter_hit, paths, says, says_refused, says_shell_refused, tells};
use crate::agent::matter_fence::{MatterFence, Other};
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
    let found = paths(&fields, &own, cfg!(windows));
    assert!(found.contains(&own.join("child/a.txt")), "{found:?}");
    assert!(found.contains(&PathBuf::from(&outside)), "{found:?}");
    assert!(found.contains(&own.join("notes/b.txt")), "{found:?}");
}

/// Claude Code's own refusals of a shell command's path: a deny rule's and the working-folder
/// block's.
const BASH_DENIED: &str = "Permission to use Bash with command cat ../B/f.txt has been denied.";
const BLOCKED: &str = "Get-Content was blocked. For security, Claude Code may only access files in \
    the allowed working directories for this session: 'C:\\Clients\\P\\A'. Attempted path: \
    'C:\\Clients\\P\\B\\f.txt'";

#[test]
fn claude_codes_shell_refusals_are_recognised() {
    for text in [
        BASH_DENIED,
        BLOCKED,
        "Permission to use PowerShell with command Get-Content x has been denied.",
    ] {
        assert!(says_shell_refused(text), "{text}");
        assert!(says(text), "{text}");
    }
    for text in [
        "Permission to read C:\\M\\Smith\\lease.pdf has been denied.",
        "cat: x: Permission denied",
        "File not found",
    ] {
        assert!(!says_shell_refused(text), "{text}");
    }
}

fn id(n: u128) -> clauseo_protocol::MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    clauseo_protocol::MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// Matters `/Clients/P/B` (not invited) and `/Clients/Q/C` (invited), the conversation working
/// in `/Clients/P/A`.
fn fence() -> (MatterFence, PathBuf) {
    let at = |path: &str| PathBuf::from(crate::testing::abs(path));
    let others = [("/Clients/P/B", false), ("/Clients/Q/C", true)]
        .iter()
        .zip(1..)
        .map(|((folder, invited), n)| {
            let mut other = Other::new(id(n), format!("M{n}"), at(folder), *invited);
            other.spellings = vec![at(folder)];
            other.paths = vec![at(folder)];
            other
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    let own = at("/Clients/P/A");
    fence.own = vec![own.clone()];
    (fence, own)
}

/// The matter a shell call's command hits, its words read with Windows' rules when `windows`
/// and names compared folded.
fn hit(command: &str, windows: bool) -> Option<clauseo_protocol::MatterId> {
    let (fence, own) = fence();
    let input = TextOrBlob::Inline(serde_json::json!({ "command": command }).to_string());
    let fields = CallFields {
        kind: None,
        status: None,
        locations: &[],
        content: &[],
        raw_input: Some(&input),
        tool_name: None,
    };
    let named = paths(&fields, &own, windows);
    matter_hit(&fence, &named, true).map(|other| other.id)
}

#[test]
fn a_shell_path_is_resolved_from_the_working_folder() {
    let b = Some(id(1));
    // Relative, as the VM's `cat ../B/f.txt` and PowerShell's `Get-Content ..\B\f.txt`.
    assert_eq!(hit("cat ../B/f.txt", false), b);
    assert_eq!(hit(r"Get-Content ..\B\f.txt", true), b);
    assert_eq!(hit("cat ./x/../../B/f.txt", false), b);
    // Absolute, and in lower case: names are compared folded.
    let lower = crate::testing::abs("/clients/p/b/f.txt");
    assert_eq!(hit(&format!("cat {lower}"), false), b);
    // A path outside every matter, or in the own one, names none.
    assert_eq!(hit("cat ../../Elsewhere/f.txt", false), None);
    assert_eq!(
        hit(
            &format!("cat {}", crate::testing::abs("/tmp/x/f.txt")),
            false
        ),
        None
    );
    assert_eq!(hit("cat ./notes/f.txt", false), None);
    assert_eq!(hit("cat ../A/f.txt", false), None);
}

/// Claude Code's own shell refusal tells the lawyer only of a matter not invited (its line
/// offers the invite); any other refusal still tells of either.
#[test]
fn a_shell_refusal_tells_only_of_a_matter_not_invited() {
    let (fence, _) = fence();
    let [b, c] = [&fence.others[0], &fence.others[1]];
    assert!(!b.invited && c.invited);
    assert!(tells(b, true, true, false));
    assert!(!tells(c, true, true, false));
    assert!(tells(c, true, false, false));
    assert!(tells(b, false, false, true));
    assert!(!tells(c, false, false, true));
    assert!(!tells(b, false, false, false));
}
