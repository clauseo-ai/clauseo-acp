//! The dialog's words, English only for now: the Hindi comes with #753, the native dialogs Hindi
//! PR. The copy page's rows are W1, W2 and W4 to W7 of S20 PR 2's word list; W3 waits for QU.8.
//! The app thread's copy page is their home; they appear nowhere else in the app. The two sets
//! are one per verdict: [`MISSING`] and [`TOO_OLD`], chosen by [`words`]. A language added later
//! is a second set of each and a pick by `ui.language`.

use super::version::Verdict;

/// One set of words for the dialog, for one verdict.
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
    /// W6: the button that opens Microsoft's `WebView2` page, "Download" or "Update". `None`: no
    /// such button, only Close.
    pub(super) action: Option<&'static str>,
    /// W7: the button that closes the dialog.
    pub(super) close: &'static str,
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

/// The dialog's width in dialog units (`TASKDIALOGCONFIG::cxWidth`), which scale with the
/// display: about 490 px at 100 %, 735 px at 150 %. At the default width the main instruction
/// wrapped before "open" (the Windows 11 VM, 2026-11-03). A test holds it to `fits`.
pub(super) const WIDTH: u32 = 280;

/// Whether `width` dialog units fit `instruction` on one line, by a rough upper bound: the main
/// instruction's larger font is under 5 dialog units a character, and the icon and margins take
/// about 50.
#[cfg(test)]
pub(super) fn fits(instruction: &str, width: u32) -> bool {
    u32::try_from(instruction.chars().count())
        .is_ok_and(|chars| chars.saturating_mul(5).saturating_add(50) <= width)
}

/// The words for a verdict that shows the dialog; `None` when the runtime starts and there is no
/// dialog.
pub(super) fn words(verdict: Verdict) -> Option<&'static Words> {
    match verdict {
        Verdict::Start => None,
        Verdict::Missing => Some(&MISSING),
        Verdict::TooOld => Some(&TOO_OLD),
    }
}

/// The runtime is missing: the button opens Microsoft's page to download it.
pub(super) const MISSING: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs Microsoft Edge WebView2 to open",
    content: "It's free from Microsoft and installs without an administrator.",
    footer: "Open Clauseo again once it's installed.",
    action: Some("Download WebView2"),
    close: "Close",
};

/// The runtime is older than the minimum. No action button, only Close: on a machine-wide
/// runtime, Microsoft's bootstrapper updates nothing for a standard user ("already installed for
/// the system", 0x80040828, the Windows 11 VM, 2026-11-09), and Windows updates it by itself.
pub(super) const TOO_OLD: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs a newer Microsoft Edge WebView2",
    content: "Windows updates it by itself.",
    footer: "Open Clauseo again once it's updated.",
    action: None,
    close: "Close",
};
