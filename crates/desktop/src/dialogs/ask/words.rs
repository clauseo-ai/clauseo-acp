//! The words of one widening question and of the picker's title, from the keys in
//! `dialogs/words.rs`. Every name, path and host goes into its slot bidi-isolated
//! (`words::isolate`), so another matter's name reads as plain words in either direction.

use clauseo_host::{Dialog, Pick};
use clauseo_protocol::Language;

use crate::dialogs::words::{Key, fill, isolate, text};

/// One question's words, in one language, its slots filled. Its `Debug` prints none of them:
/// the title and message hold names and paths.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct AskWords {
    /// The title: the task dialog's main instruction, the alert's bold line.
    pub(super) title: String,
    /// The message below it; its paragraphs apart by a blank line.
    pub(super) message: String,
    /// The allowing button where there is a checkbox (Windows).
    pub(super) allow: String,
    /// The refusing button, everywhere.
    pub(super) refuse: String,
    /// The Remember checkbox (Windows).
    pub(super) remember: String,
    /// Allow this time, where there is no checkbox (macOS and Linux).
    pub(super) allow_once: String,
    /// Allow and remember, where there is no checkbox (macOS and Linux).
    pub(super) allow_always: String,
}

/// No words: they hold names and paths.
impl std::fmt::Debug for AskWords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskWords").finish_non_exhaustive()
    }
}

impl AskWords {
    /// `dialog`'s words in `language`.
    pub(super) fn new(language: Language, dialog: &Dialog) -> Self {
        let word = |key| text(language, key).to_owned();
        let (title, message) = title_and_message(language, dialog);
        // Switching protection off confirms with its own word, on every platform: on Linux
        // it stands for Allow once, beside Always allow for this matter.
        let (allow, allow_once) = match dialog {
            Dialog::SwitchOff { .. } => (Key::AskSwitchOffConfirm, Key::AskSwitchOffConfirm),
            _ => (Key::AskAllow, Key::AskAllowOnce),
        };
        Self {
            title,
            message,
            allow: word(allow),
            refuse: word(Key::AskRefuse),
            remember: word(Key::AskRemember),
            allow_once: word(allow_once),
            allow_always: word(Key::AskAllowAlways),
        }
    }
}

/// The picker's title for `pick`, in `language`: a file's or a folder's.
pub(super) fn pick_title(language: Language, pick: &Pick) -> String {
    let key = if pick.files_too {
        Key::PickFileTitle
    } else {
        Key::PickFolderTitle
    };
    fill(
        text(language, key),
        &[("matter", &isolate(&pick.matter_name))],
    )
}

/// `dialog`'s title and message in `language`.
fn title_and_message(language: Language, dialog: &Dialog) -> (String, String) {
    let say = |key, slots: &[(&str, &str)]| fill(text(language, key), slots);
    match dialog {
        Dialog::Folder {
            matter_name,
            path,
            is_file,
            other_matter,
        } => {
            let matter = isolate(matter_name);
            let path = isolate(&path.display().to_string());
            let slots = [("matter", matter.as_str()), ("path", path.as_str())];
            let Some(other) = other_matter else {
                let title = if *is_file {
                    Key::AskFileTitle
                } else {
                    Key::AskFolderTitle
                };
                return (say(title, &[]), say(Key::AskOutsideMessage, &slots));
            };
            let (title, note) = if *is_file {
                (Key::AskOtherFileTitle, Key::AskOtherFileNote)
            } else {
                (Key::AskOtherFolderTitle, Key::AskOtherFolderNote)
            };
            let message = [
                say(note, &[("other", &isolate(other))]),
                say(Key::AskOtherMessage, &slots),
                say(Key::AskReadOnlyNote, &[]),
            ];
            (say(title, &[]), message.join("\n\n"))
        }
        Dialog::Site {
            host,
            shared_hosting,
        } => {
            let host = isolate(host);
            let slots = [("host", host.as_str())];
            let mut message = vec![say(Key::AskSiteMessage, &slots)];
            if *shared_hosting {
                message.push(say(Key::AskSharedHostingNote, &slots));
            }
            (say(Key::AskSiteTitle, &[]), message.join("\n\n"))
        }
        Dialog::SwitchOff { matter_name } => {
            let matter = isolate(matter_name);
            let slots = [("matter", matter.as_str())];
            let title = say(Key::AskSwitchOffTitle, &slots);
            (title, say(Key::AskSwitchOffMessage, &slots))
        }
        Dialog::ConfirmPick {
            matter_name,
            path,
            is_file,
            other_matter,
        } => {
            let matter = isolate(matter_name);
            let path = isolate(&path.display().to_string());
            let slots = [("matter", matter.as_str()), ("path", path.as_str())];
            let (title, note) = if *is_file {
                (Key::AskPickFileTitle, Key::AskOtherFileNote)
            } else {
                (Key::AskPickFolderTitle, Key::AskOtherFolderNote)
            };
            let mut message = Vec::new();
            if let Some(other) = other_matter {
                message.push(say(note, &[("other", &isolate(other))]));
            }
            message.push(say(Key::AskPickMessage, &slots));
            if other_matter.is_some() {
                message.push(say(Key::AskReadOnlyNote, &[]));
            }
            (say(title, &[]), message.join("\n\n"))
        }
    }
}
