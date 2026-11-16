//! The confirms' buttons: Cancel first, so it is the default and focused button, except the
//! link's, where the link's button is; only the accepting button's own label accepts.

use rfd::{MessageButtons, MessageDialogResult};

use super::*;
use crate::open::LinkWords;

/// The `(first, second)` labels `buttons` gives `ok` and `cancel` with `default`.
fn order(default: DefaultButton, ok: &str, cancel: &str) -> (String, String) {
    match buttons(default, ok, cancel) {
        MessageButtons::OkCancelCustom(first, second) => (first, second),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_default_button_comes_first() {
    let (ok, cancel) = ("Open link", "Cancel");
    let cancel_first = (cancel.to_owned(), ok.to_owned());
    let ok_first = (ok.to_owned(), cancel.to_owned());
    assert_eq!(order(DefaultButton::Cancel, ok, cancel), cancel_first);
    assert_eq!(order(DefaultButton::Ok, ok, cancel), ok_first);
}

#[test]
fn cancel_comes_first_in_always_allow_and_open_first_in_the_link_confirm() {
    // "Always allow" (U4), both languages.
    for language in [Language::En, Language::Hi] {
        let words = always_allow_words(language);
        assert_eq!(
            order(DefaultButton::Cancel, &words.ok, &words.cancel),
            (words.cancel.clone(), words.ok.clone())
        );
    }
    // The link confirm, both kinds, both languages: its button first, so Return opens.
    for words in LinkWords::every_table_for_tests() {
        assert_eq!(
            order(DefaultButton::Ok, &words.ok, &words.cancel),
            (words.ok.clone(), words.cancel.clone())
        );
        assert_ne!(words.ok, words.cancel);
    }
}

/// Only the link confirm passes `DefaultButton::Ok`; every other confirm keeps Cancel.
#[test]
fn only_the_link_confirm_makes_its_button_the_default() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let callers = [
        ("src/open/native.rs", "DefaultButton::Ok"),
        ("src/dialogs.rs", "DefaultButton::Cancel"),
        ("src/chooser/choose.rs", "DefaultButton::Cancel"),
        ("src/setup/confirm.rs", "DefaultButton::Cancel"),
        ("src/mcp.rs", "DefaultButton::Cancel"),
    ];
    for (file, default) in callers {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        let calls = text.matches("confirm(\n").count();
        assert!(calls >= 1, "{file} draws no confirm");
        assert!(text.contains(default), "{file} lacks {default}");
        if file != "src/dialogs.rs" {
            let other = if default.ends_with("Ok") {
                "DefaultButton::Cancel"
            } else {
                "DefaultButton::Ok"
            };
            assert!(!text.contains(other), "{file} passes {other}");
        }
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

/// In either order, what rfd answers for each button is that button's own label, so pressing
/// the accepting one accepts and pressing Cancel declines; Escape or closing (rfd's plain
/// `Cancel`) declines too.
#[test]
fn either_order_accepts_only_the_accepting_button() {
    for words in LinkWords::every_table_for_tests() {
        for default in [DefaultButton::Ok, DefaultButton::Cancel] {
            let (first, second) = order(default, &words.ok, &words.cancel);
            let pressed = |label: String| accepted(&MessageDialogResult::Custom(label), &words.ok);
            let (first_accepts, second_accepts) = (pressed(first), pressed(second));
            assert_eq!(first_accepts, default == DefaultButton::Ok, "{default:?}");
            assert_eq!(
                second_accepts,
                default == DefaultButton::Cancel,
                "{default:?}"
            );
            assert!(!accepted(&MessageDialogResult::Cancel, &words.ok));
        }
    }
}

/// U4's "Always allow": Escape, Alt+F4 and the close button (rfd's plain `Cancel`) decline,
/// as does its Cancel button; only "Always allow" itself grants.
#[test]
fn escape_or_close_declines_always_allow() {
    for language in [Language::En, Language::Hi] {
        let ConfirmWords { ok, cancel, .. } = always_allow_words(language);
        assert!(!accepted(&MessageDialogResult::Cancel, &ok));
        assert!(!accepted(&MessageDialogResult::Custom(cancel), &ok));
        assert!(accepted(&MessageDialogResult::Custom(ok.clone()), &ok));
    }
}

/// "Always allow" in Hindi says the Hindi words, and names Clauseo, not "the agent".
#[test]
fn always_allow_follows_the_language() {
    let hi = always_allow_words(Language::Hi);
    assert_eq!(hi.title, "हमेशा अनुमति दें?");
    assert_eq!(hi.ok, "हमेशा अनुमति दें");
    assert_eq!(hi.cancel, "रद्द करें");
    let en = always_allow_words(Language::En);
    assert_eq!(
        en.message,
        "Clauseo won't ask again before actions like this one."
    );
    assert_eq!(
        (en.ok.as_str(), en.cancel.as_str()),
        ("Always allow", "Cancel")
    );
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

/// The picker opens beside the last pick, else at home, each only when it is a non-empty
/// absolute path. The paths are not on disk: the helper must not stat them.
#[test]
fn the_picker_starts_beside_the_last_pick_or_at_home() {
    let root = Path::new(if cfg!(windows) { r"C:\" } else { "/" });
    let home = root.join("Users").join("rohan");
    let parent = home.join("Matters");
    let matter = parent.join("Smith");
    let relative = Path::new("Matters").join("Smith");
    let (home, matter, parent) = (home.as_path(), matter.as_path(), parent.as_path());
    // Beside the last pick, the parent of the folder picked.
    assert_eq!(
        start_folder(Some(matter), Some(home)),
        Some(parent.to_path_buf())
    );
    // A last pick at the root has no parent: home.
    assert_eq!(
        start_folder(Some(root), Some(home)),
        Some(home.to_path_buf())
    );
    // No last pick: home.
    assert_eq!(start_folder(None, Some(home)), Some(home.to_path_buf()));
    // A last pick whose parent is relative, or empty, is no start: home.
    assert_eq!(
        start_folder(Some(relative.as_path()), Some(home)),
        Some(home.to_path_buf())
    );
    assert_eq!(
        start_folder(Some(Path::new("")), Some(home)),
        Some(home.to_path_buf())
    );
    // A good last pick wins over an unusable home.
    assert_eq!(
        start_folder(Some(matter), Some(Path::new("rohan"))),
        Some(parent.to_path_buf())
    );
    // Neither usable: none.
    assert_eq!(start_folder(None, Some(Path::new(""))), None);
    assert_eq!(start_folder(None, Some(Path::new("Users/rohan"))), None);
    assert_eq!(
        start_folder(Some(relative.as_path()), Some(Path::new("rohan"))),
        None
    );
    assert_eq!(start_folder(None, None), None);
}

/// The MCP confirms (Settings › Tools, asked from Settings) sit over the Settings window when it
/// is open, and over main when it is closed.
#[test]
fn an_mcp_confirm_sits_over_settings_when_it_is_open() {
    assert_eq!(owner_label(Owner::SettingsOrMain, true), SETTINGS_WINDOW);
    assert_eq!(owner_label(Owner::SettingsOrMain, false), MAIN_WINDOW);
}

/// Every other confirm sits over main, whether or not Settings is open.
#[test]
fn every_other_confirm_sits_over_main() {
    for settings_open in [true, false] {
        assert_eq!(owner_label(Owner::Main, settings_open), MAIN_WINDOW);
    }
}
