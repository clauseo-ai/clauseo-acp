//! The dialog's words, in English and Hindi. They were W1, W2 and W4 to W7 of S20 PR 2's word
//! list (W3 waits for QU.8); their review home is now `ui/app/src/review/registry-nd.tsx`
//! (ND-16 to ND-21), where the Hindi is provisional, and a test fails when the two disagree.
//! They appear nowhere else in the app. The dialog shows before Tauri and the settings, so
//! [`words`] is picked by the language the app last wrote to the registry
//! (`ui_language::saved`), English when there is none.

use clauseo_protocol::Language;

/// One language's words for the dialog.
#[derive(Debug)]
pub(super) struct Words {
    /// W1: the title bar.
    pub(super) title: &'static str,
    /// W2: the main instruction, the large line.
    pub(super) instruction: &'static str,
    /// W4: the content, below it.
    pub(super) content: &'static str,
    /// W5: the footer.
    pub(super) footer: &'static str,
    /// W6: the button that opens Microsoft's download page.
    pub(super) download: &'static str,
    /// W7: the button that closes the dialog.
    pub(super) close: &'static str,
    /// The dialog's width in dialog units (`TASKDIALOGCONFIG::cxWidth`), which scale with the
    /// display: about 490 px at 100 % for 280, 735 px at 150 %. At the default width the English
    /// main instruction wrapped before "open" (the Windows 11 VM, 2026-11-03). A test holds each
    /// to `fits` its instruction.
    pub(super) width: u32,
}

impl Words {
    /// Every string, for the test that none is empty.
    #[cfg(test)]
    pub(super) fn all(&self) -> [&'static str; 6] {
        [
            self.title,
            self.instruction,
            self.content,
            self.footer,
            self.download,
            self.close,
        ]
    }
}

/// Whether `width` dialog units fit `instruction` on one line, by a rough upper bound: the main
/// instruction's larger font is under 5 dialog units a character, and the icon and margins take
/// about 50. A Devanagari vowel sign counts as a character of its own, which only overcounts.
#[cfg(test)]
pub(super) fn fits(instruction: &str, width: u32) -> bool {
    u32::try_from(instruction.chars().count())
        .is_ok_and(|chars| chars.saturating_mul(5).saturating_add(50) <= width)
}

/// The dialog's words in English.
pub(super) const EN: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs Microsoft Edge WebView2 to open",
    content: "If this computer is your firm's, its IT team may need to allow it.",
    footer: "Open Clauseo again once it's installed.",
    download: "Download WebView2",
    close: "Close",
    width: 280,
};

/// The dialog's words in Hindi, provisional for the copy review (module docs).
pub(super) const HI: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo को खुलने के लिए Microsoft Edge WebView2 चाहिए",
    content: "अगर यह कंप्यूटर आपकी फ़र्म का है, तो उसकी IT टीम को इसकी अनुमति देनी पड़ सकती है।",
    footer: "इसके इंस्टॉल होने के बाद Clauseo फिर से खोलें।",
    download: "WebView2 डाउनलोड करें",
    close: "बंद करें",
    width: 320,
};

/// The words for `language`.
pub(super) fn words(language: Language) -> &'static Words {
    match language {
        Language::En => &EN,
        Language::Hi => &HI,
    }
}
