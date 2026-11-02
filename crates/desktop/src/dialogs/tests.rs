//! The confirms' buttons: Cancel first, so it is the default and focused button, and only the
//! accepting button's own label accepts.

use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};

use super::*;
use crate::open::LinkWords;

/// The `(first, second)` labels `cancel_first` gives `ok` and `cancel`.
fn order(ok: &str, cancel: &str) -> (String, String) {
    match cancel_first(ok, cancel) {
        MessageDialogButtons::OkCancelCustom(first, second) => (first, second),
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
        MessageDialogResult::Cancel,
        MessageDialogResult::Ok,
        MessageDialogResult::Yes,
        MessageDialogResult::No,
        MessageDialogResult::Custom(String::new()),
    ] {
        assert!(!accepted(&declined, ok), "{declined:?}");
    }
}
