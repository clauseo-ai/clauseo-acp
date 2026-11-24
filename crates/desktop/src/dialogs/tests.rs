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

/// The folders a picker test starts from: a home with Documents, its matters, and the known
/// folders Windows reports, under `root` (a drive on Windows, `/` elsewhere). The paths are not
/// on disk: the choice must not stat them.
struct Fixture {
    home: PathBuf,
    matter: PathBuf,
    parent: PathBuf,
    documents: PathBuf,
    matters: PathBuf,
    known: Vec<PathBuf>,
}

impl Fixture {
    fn new() -> Self {
        let root = Path::new(if cfg!(windows) { r"C:\" } else { "/" });
        let home = root.join("Users").join("rohan");
        let documents = home.join("Documents");
        let parent = home.join("Matters");
        Self {
            matter: parent.join("Smith"),
            matters: documents.join("Clauseo"),
            known: vec![
                documents.clone(),
                home.join("AppData").join("Roaming"),
                home.join("Desktop"),
            ],
            home,
            parent,
            documents,
        }
    }

    /// The folders with the last pick and the share check's answer given, the rest as the
    /// fixture has them.
    fn folders<'a>(&'a self, last: Option<&'a Path>, stalls: Option<bool>) -> StartFolders<'a> {
        StartFolders {
            last_pick: last,
            home: Some(&self.home),
            documents: Some(&self.documents),
            matters: Some(&self.matters),
            known: &self.known,
            stalls,
        }
    }
}

/// Without a hint the picker starts beside the last pick. On Windows, else Documents once the
/// share check passed, else home; off Windows, else no folder (the OS's own remembered one).
#[test]
fn the_default_start_is_beside_the_last_pick_then_documents_then_home() {
    let fixture = Fixture::new();
    let root = Path::new(if cfg!(windows) { r"C:\" } else { "/" });
    for on_windows in [true, false] {
        // A last pick: its parent, whatever the share check says.
        for stalls in [None, Some(true), Some(false)] {
            let folders = fixture.folders(Some(&fixture.matter), stalls);
            assert_eq!(
                start_for(None, on_windows, &folders),
                Some(fixture.parent.clone()),
                "{on_windows} {stalls:?}"
            );
        }
        // A last pick at the root has no parent: the default below it.
        let folders = fixture.folders(Some(root), Some(false));
        let expected = on_windows.then(|| fixture.documents.clone());
        assert_eq!(start_for(None, on_windows, &folders), expected);
    }
    // No last pick, Windows: Documents only once the share check passed, else home.
    assert_eq!(
        start_for(None, true, &fixture.folders(None, Some(false))),
        Some(fixture.documents.clone())
    );
    for stalls in [None, Some(true)] {
        assert_eq!(
            start_for(None, true, &fixture.folders(None, stalls)),
            Some(fixture.home.clone()),
            "{stalls:?}"
        );
    }
    // No last pick, macOS (and elsewhere): no folder is set.
    for stalls in [None, Some(true), Some(false)] {
        assert_eq!(start_for(None, false, &fixture.folders(None, stalls)), None);
    }
    // Neither usable: none on Windows.
    let bare = StartFolders {
        home: None,
        documents: None,
        ..fixture.folders(None, None)
    };
    assert_eq!(start_for(None, true, &bare), None);
}

/// `"last"` and no `start` are the same choice, on both OSes and whatever the share check says.
#[test]
fn last_and_no_start_choose_the_same_folder() {
    let fixture = Fixture::new();
    for on_windows in [true, false] {
        for stalls in [None, Some(true), Some(false)] {
            for last in [Some(fixture.matter.as_path()), None] {
                let folders = fixture.folders(last, stalls);
                assert_eq!(
                    start_for(Some(&StartHint::Last), on_windows, &folders),
                    start_for(None, on_windows, &folders),
                    "{on_windows} {stalls:?} {last:?}"
                );
            }
        }
    }
}

/// Each hint opens its folder on both OSes, with no last pick and whatever the share check says
/// about a folder outside the known ones.
#[test]
fn a_hint_is_honoured_on_both_oses() {
    let fixture = Fixture::new();
    let elsewhere = fixture.home.join("Cases").join("2026");
    let hints = [
        (StartHint::Documents, fixture.documents.clone()),
        (StartHint::Matters, fixture.matters.clone()),
        (
            StartHint::Folder(elsewhere.display().to_string()),
            elsewhere.clone(),
        ),
    ];
    for on_windows in [true, false] {
        // The known folders' ones need the share check to have passed on Windows (see the
        // next test); off Windows the check never matters.
        for (hint, expected) in &hints {
            let folders = fixture.folders(None, Some(false));
            assert_eq!(
                start_for(Some(hint), on_windows, &folders),
                Some(expected.clone()),
                "{hint:?} {on_windows}"
            );
        }
        // A folder outside the known ones is honoured whatever the check says.
        let outside = &hints[2].1;
        for stalls in [None, Some(true), Some(false)] {
            let folders = fixture.folders(None, stalls);
            let hint = StartHint::Folder(outside.display().to_string());
            assert_eq!(
                start_for(Some(&hint), on_windows, &folders),
                Some(outside.clone()),
                "{on_windows} {stalls:?}"
            );
        }
        // A hint beats the last pick.
        let folders = fixture.folders(Some(&fixture.matter), Some(false));
        assert_eq!(
            start_for(Some(&StartHint::Documents), on_windows, &folders),
            Some(fixture.documents.clone())
        );
    }
}

/// On Windows a hint naming a folder inside a known folder (Documents, the default matters folder
/// under it, a Documents subfolder the path names) is not honoured unless the share check passed:
/// the default is used instead. Case and separators do not count.
#[test]
fn a_hint_inside_a_known_folder_waits_for_the_share_check() {
    let fixture = Fixture::new();
    let inside = fixture.documents.join("Smith");
    let shouted = fixture.home.join("DOCUMENTS").join("smith");
    let hints = [
        StartHint::Documents,
        StartHint::Matters,
        StartHint::Folder(inside.display().to_string()),
        StartHint::Folder(shouted.display().to_string()),
    ];
    for hint in &hints {
        for stalls in [None, Some(true)] {
            // The last pick's parent when there is one, else home (the share check has not
            // passed, so not Documents).
            let folders = fixture.folders(Some(&fixture.matter), stalls);
            assert_eq!(
                start_for(Some(hint), true, &folders),
                Some(fixture.parent.clone()),
                "{hint:?} {stalls:?}"
            );
            let folders = fixture.folders(None, stalls);
            assert_eq!(
                start_for(Some(hint), true, &folders),
                Some(fixture.home.clone()),
                "{hint:?} {stalls:?}"
            );
        }
        // Once the share check passed, the hint is honoured.
        let folders = fixture.folders(None, Some(false));
        assert!(start_for(Some(hint), true, &folders).is_some(), "{hint:?}");
    }
    // The known folder itself is inside it too: Documents, and a folder the known list names.
    let folders = fixture.folders(None, None);
    assert_eq!(
        start_for(
            Some(&StartHint::Folder(fixture.documents.display().to_string())),
            true,
            &folders
        ),
        Some(fixture.home.clone())
    );
    // A sibling that only starts with a known folder's name is not inside it.
    let sibling = fixture.home.join("Documents2");
    assert_eq!(
        start_for(
            Some(&StartHint::Folder(sibling.display().to_string())),
            true,
            &fixture.folders(None, None)
        ),
        Some(sibling)
    );
}

/// A relative or empty path is ignored, so the default is used.
#[test]
fn a_relative_or_empty_path_is_ignored() {
    let fixture = Fixture::new();
    let relative = Path::new("Matters").join("Smith").display().to_string();
    for on_windows in [true, false] {
        for path in [relative.clone(), String::new()] {
            let folders = fixture.folders(Some(&fixture.matter), Some(false));
            assert_eq!(
                start_for(Some(&StartHint::Folder(path.clone())), on_windows, &folders),
                start_for(None, on_windows, &folders),
                "{path:?} {on_windows}"
            );
        }
    }
}

/// A path that does not exist is passed through unchanged: nothing here stats it, and the dialog
/// falls back to its own default.
#[test]
fn a_missing_path_is_passed_through_unchanged() {
    let fixture = Fixture::new();
    let missing = fixture.home.join("no").join("such").join("folder");
    for on_windows in [true, false] {
        let folders = fixture.folders(None, None);
        assert_eq!(
            start_for(
                Some(&StartHint::Folder(missing.display().to_string())),
                on_windows,
                &folders
            ),
            Some(missing.clone())
        );
    }
}

/// A hint whose folder the host or the OS did not give is no hint: the default is used.
#[test]
fn a_hint_with_no_folder_falls_back_to_the_default() {
    let fixture = Fixture::new();
    let no_matters = StartFolders {
        matters: None,
        ..fixture.folders(Some(&fixture.matter), None)
    };
    assert_eq!(
        start_for(Some(&StartHint::Matters), false, &no_matters),
        Some(fixture.parent.clone())
    );
    let no_documents = StartFolders {
        documents: None,
        ..fixture.folders(None, None)
    };
    assert_eq!(
        start_for(Some(&StartHint::Documents), false, &no_documents),
        None
    );
}

/// Where the host's `GetDefaultLocation` display is: absolute as it is, below home as home joined
/// with it, and nothing for an empty display or a relative one with no home.
#[test]
fn the_matters_display_is_a_folder() {
    let fixture = Fixture::new();
    assert_eq!(
        matters_from_display("Documents/Clauseo", Some(&fixture.home)),
        Some(fixture.matters.clone())
    );
    assert_eq!(
        matters_from_display(&fixture.matters.display().to_string(), None),
        Some(fixture.matters.clone())
    );
    assert_eq!(matters_from_display("", Some(&fixture.home)), None);
    assert_eq!(matters_from_display("Documents/Clauseo", None), None);
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
