//! The dialog's words, English only (Rohan, 2026-11-03: new words ship in English for now). The
//! copy page's rows are W1, W2 and W4 to W7 of S20 PR 2's word list; W3 waits for QU.8. The app
//! thread's copy page is their home; they appear nowhere else in the app. A language added later
//! is a second `Words` and a pick by `ui.language`.

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

/// The dialog's words.
pub(super) const EN: Words = Words {
    title: "Clauseo",
    instruction: "Clauseo needs Microsoft Edge WebView2 to open",
    content: "If this computer is your firm's, its IT team may need to allow it.",
    footer: "Open Clauseo again once it's installed.",
    download: "Download WebView2",
    close: "Close",
};
