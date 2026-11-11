//! `open_privacy_settings`: macOS's Files and Folders pane, and nothing off macOS.

use clauseo_protocol::ErrorCode;

use super::{FakeOpener, Opened};
use crate::open::{PRIVACY_FILES_AND_FOLDERS, open_privacy_settings};

/// On macOS it opens the Files and Folders pane, that URL only; elsewhere it opens nothing and
/// is `command.invalid`.
#[test]
fn open_privacy_settings_opens_the_files_and_folders_pane_on_macos_only() {
    assert_eq!(
        PRIVACY_FILES_AND_FOLDERS,
        "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders"
    );
    let opener = FakeOpener::default();
    open_privacy_settings(&opener, true).unwrap();
    let pane = Opened::Url(PRIVACY_FILES_AND_FOLDERS.to_owned());
    assert_eq!(opener.opened(), [pane]);

    let elsewhere = FakeOpener::default();
    let error = open_privacy_settings(&elsewhere, false).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid);
    assert_eq!(elsewhere.opened(), []);
}
