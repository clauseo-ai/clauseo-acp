//! Which network matters are stored by their network address, so the host answers their reads.

use std::path::Path;

use clauseo_protocol::FolderSupportKind;

use super::by_address;
use crate::agent::folder_support::Answer;

const NETWORK: Option<Answer> = Some(Answer::Known(Some(FolderSupportKind::Network)));

fn spelled(text: &str) -> bool {
    by_address(NETWORK, Path::new(text), true)
}

/// By its address, in either slash or the verbatim UNC form.
#[test]
fn a_matter_stored_by_its_address_is_by_address() {
    for text in [
        r"\\server\share\Matters\A",
        "//server/share/Matters/A",
        r"\\?\UNC\server\share\Matters\A",
    ] {
        assert!(spelled(text), "{text}");
    }
}

/// A drive letter (mapped or not), and a drive by its verbatim spelling, are not.
#[test]
fn a_matter_on_a_drive_letter_is_not() {
    for text in [r"Z:\Matters\A", "Z:/Matters/A", r"Z:\", r"\\?\Z:\Matters\A"] {
        assert!(!spelled(text), "{text}");
    }
}

/// A local folder, a check that did not run or failed, and every other OS: never.
#[test]
fn only_a_network_answer_on_windows_counts() {
    let matter = Path::new(r"\\server\share\Matters\A");
    for answer in [
        None,
        Some(Answer::Known(None)),
        Some(Answer::Failed),
        Some(Answer::Known(Some(FolderSupportKind::NoPermissions))),
        Some(Answer::Known(Some(
            FolderSupportKind::CantChangePermissions,
        ))),
    ] {
        assert!(!by_address(answer, matter, true), "{answer:?}");
    }
    assert!(!by_address(NETWORK, matter, false));
}
