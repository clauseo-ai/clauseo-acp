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
