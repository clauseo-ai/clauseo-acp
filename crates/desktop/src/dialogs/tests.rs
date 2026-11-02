//! The confirms' buttons: Cancel first, so it is the default and focused button, and only the
//! accepting button's own label accepts.

use rfd::{MessageButtons, MessageDialogResult};

use super::*;
use crate::open::LinkWords;

/// The `(first, second)` labels `cancel_first` gives `ok` and `cancel`.
fn order(ok: &str, cancel: &str) -> (String, String) {
    match cancel_first(ok, cancel) {
        MessageButtons::OkCancelCustom(first, second) => (first, second),
        other => panic!("{other:?}"),
    }
}

#[test]
fn cancel_comes_first_in_both_confirms() {
    // "Always allow" (U4).
    assert_eq!(
        order(CONFIRM_OK, CONFIRM_CANCEL),
        (CONFIRM_CANCEL.to_owned(), CONFIRM_OK.to_owned())
    );
    // The link confirm, both kinds, both languages.
    for words in LinkWords::every_table_for_tests() {
        assert_eq!(
            order(&words.ok, &words.cancel),
            (words.cancel.clone(), words.ok.clone())
        );
        assert_ne!(words.ok, words.cancel);
    }
}

#[test]
fn only_the_accepting_button_accepts() {
    let ok = "Open link";
    assert!(accepted(&MessageDialogResult::Custom(ok.to_owned()), ok));
    for declined in [
        MessageDialogResult::Custom("Cancel".to_owned()),
        // rfd's answer for Escape, Alt+F4, the close button and a failed dialog.
        MessageDialogResult::Cancel,
        MessageDialogResult::Ok,
        MessageDialogResult::Yes,
        MessageDialogResult::No,
        MessageDialogResult::Custom(String::new()),
    ] {
        assert!(!accepted(&declined, ok), "{declined:?}");
    }
}

/// U4's "Always allow": Escape, Alt+F4 and the close button (rfd's plain `Cancel`) decline,
/// as does its Cancel button; only "Always allow" itself grants.
#[test]
fn escape_or_close_declines_always_allow() {
    assert!(!accepted(&MessageDialogResult::Cancel, CONFIRM_OK));
    assert!(!accepted(
        &MessageDialogResult::Custom(CONFIRM_CANCEL.to_owned()),
        CONFIRM_OK
    ));
    assert!(accepted(
        &MessageDialogResult::Custom(CONFIRM_OK.to_owned()),
        CONFIRM_OK
    ));
}

/// The confirms never go through the dialog plugin's message dialog, which turns rfd's plain
/// `Cancel` (Escape, close) into the second label: with Cancel first, the accepting one.
#[test]
fn the_confirms_never_use_the_plugins_message_dialog() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["src/dialogs.rs", "src/open/native.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        for call in [
            ".dialog().message(",
            ".message_dialog(",
            "show_with_result(",
        ] {
            assert!(!text.contains(call), "{file} calls {call}");
        }
    }
}

/// On Linux the confirms rely on rfd's gtk3 backend, where Escape is a plain `Cancel`. With
/// tauri-plugin-dialog's `xdg-portal` feature rfd would use the portal and zenity, which answers
/// Escape with the second label (accept). Cargo.lock lists the dependencies of the features in
/// force, so rfd there must take gtk and not ashpd (the portal).
#[test]
fn rfd_keeps_its_gtk_backend_and_not_the_portal() {
    let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(lock).unwrap();
    let start = lock.find("\nname = \"rfd\"\n").unwrap();
    let entry = &lock[start..];
    let end = entry[1..]
        .find("\n[[package]]")
        .map_or(entry.len(), |end| end + 1);
    let entry = &entry[..end];
    assert!(
        entry.contains("\"gtk-sys\""),
        "rfd lost its gtk3 backend:{entry}"
    );
    assert!(
        !entry.contains("\"ashpd\""),
        "rfd takes the xdg portal:{entry}"
    );
}
