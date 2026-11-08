//! The widening questions: the answer from each platform's buttons and checkbox, Escape
//! refusing, the words of each dialog with another matter's name isolated, and the script.

use std::path::PathBuf;

use clauseo_host::{Dialog, DialogAnswer, NativeDialogs, Pick};
use clauseo_protocol::Language;
use rfd::{MessageButtons, MessageDialogResult};

use super::answer::{
    ALERT_ALLOW, ALERT_REFUSE, ALLOW, Press, REFUSE, ThreeButtons, alert_press, from_press,
    task_dialog_press,
};
use super::script::Scripted;
use super::words::{AskWords, pick_title};

/// Win32's IDCANCEL: Escape, Alt+F4 and the close button.
const IDCANCEL: i32 = 2;

/// `NSModalResponseStop`, `NSModalResponseAbort` and `NSModalResponseCancel`: a modal session
/// ended by something other than a button.
const ALERT_ENDED_OTHERWISE: [isize; 3] = [-1000, -1001, 0];

#[test]
fn a_press_and_the_checkbox_give_the_answer() {
    assert_eq!(from_press(Press::Allow, true), DialogAnswer::AllowForMatter);
    assert_eq!(from_press(Press::Allow, false), DialogAnswer::AllowOnce);
    assert_eq!(
        from_press(Press::Refuse, true),
        DialogAnswer::RefuseForMatter
    );
    assert_eq!(from_press(Press::Refuse, false), DialogAnswer::Refuse);
    assert_eq!(from_press(Press::Neither, true), DialogAnswer::Refuse);
    assert_eq!(from_press(Press::Neither, false), DialogAnswer::Refuse);
}

#[test]
fn the_task_dialog_maps_button_and_checkbox() {
    let answer = |pressed, remember| from_press(task_dialog_press(pressed), remember);
    assert_eq!(answer(ALLOW, true), DialogAnswer::AllowForMatter);
    assert_eq!(answer(ALLOW, false), DialogAnswer::AllowOnce);
    assert_eq!(answer(REFUSE, true), DialogAnswer::RefuseForMatter);
    assert_eq!(answer(REFUSE, false), DialogAnswer::Refuse);
}

/// Escape, Alt+F4 and the close button (IDCANCEL), and no button at all, refuse, and never
/// for the matter, whatever the checkbox says.
#[test]
fn escape_refuses_on_windows_whatever_the_checkbox() {
    for remember in [false, true] {
        for pressed in [IDCANCEL, 0, 1, -1, 102] {
            let answer = from_press(task_dialog_press(pressed), remember);
            assert_eq!(answer, DialogAnswer::Refuse);
        }
    }
}

/// macOS: Don't allow is the first button added (on Escape), Allow the second; the suppression
/// checkbox maps as Windows' checkbox does, Escape included (it presses Don't allow).
#[test]
fn the_alert_maps_button_and_checkbox() {
    assert_eq!(ALERT_REFUSE, 1000);
    assert_eq!(ALERT_ALLOW, 1001);
    let answer = |response, remember| from_press(alert_press(response), remember);
    assert_eq!(answer(ALERT_ALLOW, true), DialogAnswer::AllowForMatter);
    assert_eq!(answer(ALERT_ALLOW, false), DialogAnswer::AllowOnce);
    assert_eq!(answer(ALERT_REFUSE, true), DialogAnswer::RefuseForMatter);
    assert_eq!(answer(ALERT_REFUSE, false), DialogAnswer::Refuse);
}

/// A modal session the alert's buttons did not end (stopped, aborted, cancelled: the app
/// quitting) refuses whatever the checkbox says.
#[test]
fn an_alert_ended_otherwise_refuses_on_macos_whatever_the_checkbox() {
    for remember in [false, true] {
        for response in ALERT_ENDED_OTHERWISE.into_iter().chain([1002, 1]) {
            let answer = from_press(alert_press(response), remember);
            assert_eq!(answer, DialogAnswer::Refuse, "{response}");
        }
    }
}

/// The labels `buttons` draws, in order.
fn drawn(buttons: &ThreeButtons) -> [String; 3] {
    match buttons.buttons() {
        MessageButtons::YesNoCancelCustom(first, second, third) => [first, second, third],
        other => panic!("{other:?}"),
    }
}

fn three() -> ThreeButtons {
    ThreeButtons::new("Once", "Always", "No")
}

/// Linux: the gtk dialog awaited answers the positions' plain results, and its plain `Cancel`
/// is both the third button and Escape, so the third is Don't allow.
#[test]
fn linux_draws_dont_allow_third_and_maps_positions_and_labels() {
    let buttons = three();
    assert_eq!(drawn(&buttons), ["Once", "Always", "No"]);
    assert_eq!(
        buttons.answer(&MessageDialogResult::Yes),
        DialogAnswer::AllowOnce
    );
    assert_eq!(
        buttons.answer(&MessageDialogResult::No),
        DialogAnswer::AllowForMatter
    );
    let custom = |label: &str| buttons.answer(&MessageDialogResult::Custom(label.to_owned()));
    assert_eq!(custom("Once"), DialogAnswer::AllowOnce);
    assert_eq!(custom("Always"), DialogAnswer::AllowForMatter);
    assert_eq!(custom("No"), DialogAnswer::Refuse);
}

/// rfd's plain `Cancel` (Escape, the close button, a stopped or failed dialog), `Ok` and an
/// unknown label refuse; nothing refuses for the matter (rfd has no checkbox).
#[test]
fn escape_refuses_with_rfd() {
    let buttons = three();
    for result in [
        MessageDialogResult::Cancel,
        MessageDialogResult::Ok,
        MessageDialogResult::Custom(String::new()),
        MessageDialogResult::Custom("Allow".to_owned()),
    ] {
        assert_eq!(buttons.answer(&result), DialogAnswer::Refuse, "{result:?}");
    }
}

fn every_dialog() -> Vec<Dialog> {
    let path = PathBuf::from("/Users/rohan/Matters/Brown/brief.docx");
    let mut dialogs = Vec::new();
    for is_file in [false, true] {
        for other_matter in [None, Some("Brown".to_owned())] {
            let matter_name = "Smith".to_owned();
            let (path, other) = (path.clone(), other_matter.clone());
            dialogs.push(Dialog::Folder {
                matter_name: matter_name.clone(),
                path: path.clone(),
                is_file,
                other_matter: other.clone(),
            });
            dialogs.push(Dialog::ConfirmPick {
                matter_name,
                path,
                is_file,
                other_matter: other,
            });
        }
    }
    for shared_hosting in [false, true] {
        let host = "court.example".to_owned();
        dialogs.push(Dialog::Site {
            host,
            shared_hosting,
        });
    }
    dialogs.push(Dialog::SwitchOff {
        matter_name: "Smith".to_owned(),
    });
    dialogs
}

/// Every dialog has a title, a message and distinct buttons in both languages, and leaves no
/// slot unfilled.
#[test]
fn every_dialog_has_its_words() {
    for language in [Language::En, Language::Hi] {
        for dialog in every_dialog() {
            let words = AskWords::new(language, &dialog);
            for text in [&words.title, &words.message] {
                assert!(!text.trim().is_empty(), "{dialog:?}");
                assert!(!text.contains('{') && !text.contains('}'), "{text}");
            }
            let labels = [&words.allow_once, &words.allow_always, &words.refuse];
            assert!(labels[0] != labels[1] && labels[1] != labels[2] && labels[0] != labels[2]);
            assert_ne!(words.allow, words.refuse);
            assert_ne!(words.remember, "");
        }
    }
}

/// Another matter's name is said in plain words, isolated, in the folder, file and pick
/// dialogs; so are the matter's own name and the path.
#[test]
fn another_matters_name_is_isolated() {
    let other = "\u{2068}Brown\u{2069}";
    let folder = Dialog::Folder {
        matter_name: "Smith".to_owned(),
        path: PathBuf::from("/m/Brown/Pleadings"),
        is_file: false,
        other_matter: Some("Brown".to_owned()),
    };
    let words = AskWords::new(Language::En, &folder);
    assert!(
        words
            .message
            .starts_with(&format!("This folder belongs to another matter, {other}.")),
        "{}",
        words.message
    );
    assert!(words.message.contains("\u{2068}Smith\u{2069}"));
    assert!(words.message.contains("\u{2068}/m/Brown/Pleadings\u{2069}"));
    let pick = Dialog::ConfirmPick {
        matter_name: "Smith".to_owned(),
        path: PathBuf::from("/m/Brown/brief.docx"),
        is_file: true,
        other_matter: Some("Brown\u{2069}\u{202E}x".to_owned()),
    };
    let words = AskWords::new(Language::En, &pick);
    // Its own PDI and override are gone: it cannot close the isolate.
    let note = "This file belongs to another matter, \u{2068}Brownx\u{2069}.";
    assert!(words.message.starts_with(note), "{}", words.message);
}

/// Without another matter there is no line about one; with a shared server there is one more.
#[test]
fn the_message_follows_the_dialog() {
    let outside = Dialog::Folder {
        matter_name: "Smith".to_owned(),
        path: PathBuf::from("/m/Elsewhere"),
        is_file: false,
        other_matter: None,
    };
    let words = AskWords::new(Language::En, &outside);
    assert_eq!(words.title, "Let Clauseo read this folder?");
    assert!(!words.message.contains("another matter"));
    let site = |shared_hosting| Dialog::Site {
        host: "court.example".to_owned(),
        shared_hosting,
    };
    let plain = AskWords::new(Language::En, &site(false)).message;
    let shared = AskWords::new(Language::En, &site(true)).message;
    assert!(shared.starts_with(&plain) && shared.len() > plain.len());
}

/// Switching protection off confirms with "Switch off" (Hindi "बंद करें") in place of Allow, on
/// every platform; Don't allow and the checkbox stay.
#[test]
fn switching_off_confirms_with_its_own_word() {
    let off = Dialog::SwitchOff {
        matter_name: "Smith".to_owned(),
    };
    let en = AskWords::new(Language::En, &off);
    assert_eq!(
        (en.allow.as_str(), en.allow_once.as_str()),
        ("Switch off", "Switch off")
    );
    assert_eq!(en.refuse, "Don't allow");
    assert_eq!(en.remember, "Remember for this matter");
    let hi = AskWords::new(Language::Hi, &off);
    assert_eq!(
        (hi.allow.as_str(), hi.allow_once.as_str()),
        ("बंद करें", "बंद करें")
    );
    let site = Dialog::Site {
        host: "court.example".to_owned(),
        shared_hosting: false,
    };
    let other = AskWords::new(Language::En, &site);
    assert_eq!(
        (other.allow.as_str(), other.allow_once.as_str()),
        ("Allow", "Allow once")
    );
}

#[test]
fn the_picker_title_names_the_matter() {
    let pick = |files_too| Pick {
        start: PathBuf::from("/m"),
        files_too,
        matter_name: "Smith".to_owned(),
    };
    let folder = pick_title(Language::En, &pick(false));
    assert_eq!(folder, "Choose a folder for \u{2068}Smith\u{2069}");
    let file = pick_title(Language::En, &pick(true));
    assert_eq!(file, "Choose a file for \u{2068}Smith\u{2069}");
}

#[test]
fn the_script_answers_in_order_then_refuses() {
    use DialogAnswer::{AllowForMatter, AllowOnce, Refuse, RefuseForMatter};
    let words = Some("allow_once, allow_for_matter,refuse_for_matter,refuse,typo");
    let picks = vec![PathBuf::from("/m/Brown"), PathBuf::new()];
    let script = Scripted::new(words, picks);
    let off = || Dialog::SwitchOff {
        matter_name: "Smith".to_owned(),
    };
    let answers: Vec<_> = (0..6).map(|_| script.ask(off())).collect();
    let expected = [
        AllowOnce,
        AllowForMatter,
        RefuseForMatter,
        Refuse,
        Refuse,
        Refuse,
    ];
    assert_eq!(answers, expected);
    let pick = || Pick {
        start: PathBuf::from("/m"),
        files_too: false,
        matter_name: "Smith".to_owned(),
    };
    assert_eq!(script.pick(pick()), Some(PathBuf::from("/m/Brown")));
    // An empty entry is a cancelled picker; past the script, cancelled too.
    assert_eq!(script.pick(pick()), None);
    assert_eq!(script.pick(pick()), None);
    let empty = Scripted::new(None, Vec::new());
    assert_eq!(empty.ask(off()), Refuse);
}

/// The words' and the script's `Debug` print no name, path or host.
#[test]
fn debug_prints_no_name_or_path() {
    for dialog in every_dialog() {
        let words = AskWords::new(Language::En, &dialog);
        let debug = format!("{words:?}");
        for secret in ["Smith", "Brown", "brief", "court"] {
            assert!(!debug.contains(secret), "{debug}");
        }
    }
    let script = Scripted::new(Some("allow_once"), vec![PathBuf::from("/m/Brown")]);
    let debug = format!("{script:?}");
    assert!(!debug.contains("Brown"), "{debug}");
    assert!(debug.contains("AllowOnce"), "{debug}");
}
