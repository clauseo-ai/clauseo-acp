//! The words of the Mac's menu bar (`menu.rs`), and of Quit Clauseo in Windows' system menu
//! (MN-07, `window_frame/quit_item.rs`), in English and Hindi by the app's language
//! (`ui.language`), as the native dialogs' (`dialogs/words.rs`). Their review home is
//! `ui/app/src/review/registry-mn.tsx` (MN-01 to MN-25), where the Hindi is provisional; a test
//! fails when the two disagree. The items macOS adds itself (autofill, Start Dictation, Emoji &
//! Symbols, the Services list, Help's search) and the full-screen item's retitle while in full
//! screen are the system's, in the computer's language.

use clauseo_protocol::Language;

/// One word of the menu bar.
#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    /// MN-01: the app menu's About.
    About,
    /// MN-02: the app menu's Settings, which opens the settings window.
    Settings,
    /// MN-03: the app menu's Services.
    Services,
    /// MN-04: Hide Clauseo.
    Hide,
    /// MN-05: Hide Others.
    HideOthers,
    /// MN-06: Show All.
    ShowAll,
    /// MN-07: Quit Clauseo.
    Quit,
    /// MN-08: the File menu.
    File,
    /// MN-09: New conversation, in the open matter.
    NewConversation,
    /// MN-10: New matter, its dialog.
    NewMatter,
    /// MN-11: Close Window, in File and in Window.
    CloseWindow,
    /// MN-12: the Edit menu.
    Edit,
    /// MN-13: Undo.
    Undo,
    /// MN-14: Redo.
    Redo,
    /// MN-15: Cut.
    Cut,
    /// MN-16: Copy.
    Copy,
    /// MN-17: Paste.
    Paste,
    /// MN-18: Select All.
    SelectAll,
    /// MN-19: Find, the rail's.
    Find,
    /// MN-20: the View menu.
    View,
    /// MN-21: Enter Full Screen.
    FullScreen,
    /// MN-22: the Window menu.
    Window,
    /// MN-23: Minimize.
    Minimize,
    /// MN-24: Zoom.
    Zoom,
    /// MN-25: the Help menu.
    Help,
}

#[cfg(any(test, target_os = "macos"))]
impl Key {
    /// Every key, in review order (MN-01 first).
    #[cfg(test)]
    pub(crate) const ALL: [Self; 25] = [
        Self::About,
        Self::Settings,
        Self::Services,
        Self::Hide,
        Self::HideOthers,
        Self::ShowAll,
        Self::Quit,
        Self::File,
        Self::NewConversation,
        Self::NewMatter,
        Self::CloseWindow,
        Self::Edit,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::Find,
        Self::View,
        Self::FullScreen,
        Self::Window,
        Self::Minimize,
        Self::Zoom,
        Self::Help,
    ];
}

/// `key`'s words in `language`.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn text(language: Language, key: Key) -> &'static str {
    match language {
        Language::En => en(key),
        Language::Hi => hi(key),
    }
}

/// MN-07, Quit Clauseo, in `language`: the Mac bar's Quit and Windows' system-menu item
/// (`window_frame/quit_item.rs`), the only word Windows takes from here.
pub(crate) fn quit(language: Language) -> &'static str {
    match language {
        Language::En => "Quit Clauseo",
        Language::Hi => "Clauseo बंद करें",
    }
}

#[cfg(any(test, target_os = "macos"))]
fn en(key: Key) -> &'static str {
    match key {
        Key::About => "About Clauseo",
        Key::Settings => "Settings…",
        Key::Services => "Services",
        Key::Hide => "Hide Clauseo",
        Key::HideOthers => "Hide Others",
        Key::ShowAll => "Show All",
        Key::Quit => quit(Language::En),
        Key::File => "File",
        Key::NewConversation => "New conversation",
        Key::NewMatter => "New matter",
        Key::CloseWindow => "Close Window",
        Key::Edit => "Edit",
        Key::Undo => "Undo",
        Key::Redo => "Redo",
        Key::Cut => "Cut",
        Key::Copy => "Copy",
        Key::Paste => "Paste",
        Key::SelectAll => "Select All",
        Key::Find => "Find",
        Key::View => "View",
        Key::FullScreen => "Enter Full Screen",
        Key::Window => "Window",
        Key::Minimize => "Minimize",
        Key::Zoom => "Zoom",
        Key::Help => "Help",
    }
}

/// Provisional Hindi, for the copy review (module docs). The room's own words where it has
/// them: नई बातचीत, नया मामला, खोजें, सेटिंग्स, पूर्ववत करें.
#[cfg(any(test, target_os = "macos"))]
fn hi(key: Key) -> &'static str {
    match key {
        Key::About => "Clauseo के बारे में",
        Key::Settings => "सेटिंग्स…",
        Key::Services => "सेवाएँ",
        Key::Hide => "Clauseo छिपाएँ",
        Key::HideOthers => "बाकी छिपाएँ",
        Key::ShowAll => "सब दिखाएँ",
        Key::Quit => quit(Language::Hi),
        Key::File => "फ़ाइल",
        Key::NewConversation => "नई बातचीत",
        Key::NewMatter => "नया मामला",
        Key::CloseWindow => "विंडो बंद करें",
        Key::Edit => "संपादन",
        Key::Undo => "पूर्ववत करें",
        Key::Redo => "फिर से करें",
        Key::Cut => "काटें",
        Key::Copy => "कॉपी करें",
        Key::Paste => "पेस्ट करें",
        Key::SelectAll => "सब चुनें",
        Key::Find => "खोजें",
        Key::View => "देखें",
        Key::FullScreen => "पूरी स्क्रीन पर देखें",
        Key::Window => "विंडो",
        Key::Minimize => "छोटा करें",
        Key::Zoom => "ज़ूम करें",
        Key::Help => "सहायता",
    }
}

#[cfg(test)]
mod tests;
