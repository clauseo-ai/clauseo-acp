//! The dialog's words, one set per verdict and per language: [`MISSING_EN`], [`MISSING_HI`],
//! [`TOO_OLD_EN`] and [`TOO_OLD_HI`], chosen by [`words`]. They were W1, W2 and W4 to W7 of S20
//! PR 2's word list (W3 waits for QU.8); their review home is now
//! `ui/app/src/review/registry-nd.tsx` (ND-16 to ND-21 for Missing, ND-22 to ND-24 for
//! `TooOld`'s own lines), where the Hindi is provisional, and a test fails when the two
//! disagree. They appear nowhere else in the app. The dialog shows before Tauri and the
//! settings, so the language is the one the app last wrote to the registry
//! (`ui_language::saved`), English when there is none.

use clauseo_protocol::Language;

use super::version::Verdict;

/// One set of words for the dialog, for one verdict in one language.
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
    /// W6: the button that opens Microsoft's `WebView2` page. `None`: no such button, only
    /// Close.
    pub(super) action: Option<&'static str>,
    /// W7: the button that closes the dialog.
    pub(super) close: &'static str,
    /// The dialog's width in dialog units (`TASKDIALOGCONFIG::cxWidth`), which scale with the
    /// display: about 490 px at 100 % for 280, 735 px at 150 %. At the default width the English
    /// main instruction wrapped before "open" (the Windows 11 VM, 2026-11-03). One width per
    /// language; a test holds each set's to `fits` its instruction.
    pub(super) width: u32,
}

impl Words {
    /// Every string, for the test that none is empty.
    #[cfg(test)]
    pub(super) fn all(&self) -> Vec<&'static str> {
        [
            Some(self.title),
            Some(self.instruction),
            Some(self.content),
            Some(self.footer),
            self.action,
            Some(self.close),
        ]
        .into_iter()
        .flatten()
        .collect()
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

/// The English sets' width.
const WIDTH_EN: u32 = 280;

/// The Hindi sets' width: `TooOld`'s instruction is the longer line.
const WIDTH_HI: u32 = 330;

/// The words for a verdict that shows the dialog, in `language`; `None` when the runtime starts
/// and there is no dialog.
pub(super) fn words(verdict: Verdict, language: Language) -> Option<&'static Words> {
    match (verdict, language) {
        (Verdict::Start, _) => None,
        (Verdict::Missing, Language::En) => Some(&MISSING_EN),
        (Verdict::Missing, Language::Hi) => Some(&MISSING_HI),
        (Verdict::TooOld, Language::En) => Some(&TOO_OLD_EN),
        (Verdict::TooOld, Language::Hi) => Some(&TOO_OLD_HI),
    }
}

/// The runtime is missing: the button opens Microsoft's page to download it.
pub(super) const MISSING_EN: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs Microsoft Edge WebView2 to open",
    content: "It's free from Microsoft and installs without an administrator.",
    footer: "Open Clauseo again once it's installed.",
    action: Some("Download WebView2"),
    close: "Close",
    width: WIDTH_EN,
};

/// [`MISSING_EN`] in Hindi, provisional for the copy review (module docs).
pub(super) const MISSING_HI: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo को खुलने के लिए Microsoft Edge WebView2 चाहिए",
    content: "यह Microsoft से मुफ़्त है और बिना व्यवस्थापक के इंस्टॉल होता है।",
    footer: "इंस्टॉल होने के बाद Clauseo फिर से खोलें।",
    action: Some("WebView2 डाउनलोड करें"),
    close: "बंद करें",
    width: WIDTH_HI,
};

/// The runtime is older than the minimum. No action button, only Close: on a machine-wide
/// runtime, Microsoft's bootstrapper updates nothing for a standard user ("already installed for
/// the system", 0x80040828, the Windows 11 VM, 2026-11-09), and Windows updates it by itself.
pub(super) const TOO_OLD_EN: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs a newer Microsoft Edge WebView2",
    content: "Windows updates it by itself.",
    footer: "Open Clauseo again once it's updated.",
    action: None,
    close: "Close",
    width: WIDTH_EN,
};

/// [`TOO_OLD_EN`] in Hindi, provisional for the copy review (module docs).
pub(super) const TOO_OLD_HI: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo को Microsoft Edge WebView2 का नया संस्करण चाहिए",
    content: "Windows इसे अपने आप अपडेट करता है।",
    footer: "अपडेट होने के बाद Clauseo फिर से खोलें।",
    action: None,
    close: "बंद करें",
    width: WIDTH_HI,
};
