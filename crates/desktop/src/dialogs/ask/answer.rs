//! The lawyer's answer from what a dialog returned, as pure functions: the two-button dialogs
//! with a Remember checkbox (Windows' task dialog, macOS' `NSAlert`), and rfd's three buttons
//! (Linux). Anything but a press of an allowing button refuses. Escape or the close button
//! refuses whatever the checkbox says, except on macOS, where Escape presses Don't allow and the
//! alert can't tell the two apart.

use clauseo_host::DialogAnswer;
#[cfg(any(all(unix, not(target_os = "macos")), test))]
use rfd::{MessageButtons, MessageDialogResult};

/// Which button of a two-button dialog was pressed.
#[cfg(any(windows, target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Press {
    /// Allow.
    Allow,
    /// Don't allow.
    Refuse,
    /// Neither: Escape, the close button, a stopped or failed dialog.
    Neither,
}

/// The answer for `press` with the checkbox `remember`ed: Allow remembered is for the matter,
/// Allow alone once, Don't allow remembered refuses for the matter, and anything else refuses,
/// checked or not.
#[cfg(any(windows, target_os = "macos", test))]
pub(super) fn from_press(press: Press, remember: bool) -> DialogAnswer {
    match (press, remember) {
        (Press::Allow, true) => DialogAnswer::AllowForMatter,
        (Press::Allow, false) => DialogAnswer::AllowOnce,
        (Press::Refuse, true) => DialogAnswer::RefuseForMatter,
        (Press::Refuse | Press::Neither, _) => DialogAnswer::Refuse,
    }
}

/// The task dialog's Allow button.
#[cfg(any(windows, test))]
pub(super) const ALLOW: i32 = 100;
/// Its Don't allow button, the default.
#[cfg(any(windows, test))]
pub(super) const REFUSE: i32 = 101;

/// The task dialog's `pressed` button: `IDCANCEL` (Escape, Alt+F4, the close button) or no
/// button is neither.
#[cfg(any(windows, test))]
pub(super) fn task_dialog_press(pressed: i32) -> Press {
    match pressed {
        ALLOW => Press::Allow,
        REFUSE => Press::Refuse,
        _ => Press::Neither,
    }
}

/// `NSAlertFirstButtonReturn`: the first button added, Don't allow, on Escape.
#[cfg(any(target_os = "macos", test))]
pub(super) const ALERT_REFUSE: isize = 1000;
/// `NSAlertSecondButtonReturn`: the second button added, Allow.
#[cfg(any(target_os = "macos", test))]
pub(super) const ALERT_ALLOW: isize = 1001;

/// `runModal`'s `response`: any other (a stopped or aborted modal session) is neither.
#[cfg(any(target_os = "macos", test))]
pub(super) fn alert_press(response: isize) -> Press {
    match response {
        ALERT_ALLOW => Press::Allow,
        ALERT_REFUSE => Press::Refuse,
        _ => Press::Neither,
    }
}

/// Linux's three buttons, each with its answer, in the order drawn: Allow once, Always allow for
/// this matter, Don't allow. rfd's gtk dialog awaited answers the plain `Yes`, `No` and `Cancel`
/// of the three positions, and `Cancel` is also Escape and the close button, so Don't allow
/// comes third.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThreeButtons([(String, DialogAnswer); 3]);

#[cfg(any(all(unix, not(target_os = "macos")), test))]
impl ThreeButtons {
    /// The buttons labelled `once`, `always` and `refuse`.
    pub(super) fn new(once: &str, always: &str, refuse: &str) -> Self {
        Self([
            (once.to_owned(), DialogAnswer::AllowOnce),
            (always.to_owned(), DialogAnswer::AllowForMatter),
            (refuse.to_owned(), DialogAnswer::Refuse),
        ])
    }

    /// The buttons as rfd draws them.
    pub(super) fn buttons(&self) -> MessageButtons {
        let [first, second, third] = &self.0;
        MessageButtons::YesNoCancelCustom(first.0.clone(), second.0.clone(), third.0.clone())
    }

    /// The answer for rfd's `result`: a button's own label (`Custom`), or the plain `Yes` or
    /// `No` of the first or second position (the gtk dialog awaited). Anything else, its plain
    /// `Cancel` above all (Escape, the close button, a stopped or failed dialog), refuses.
    pub(super) fn answer(&self, result: &MessageDialogResult) -> DialogAnswer {
        let [first, second, _] = &self.0;
        match result {
            MessageDialogResult::Custom(label) => self
                .0
                .iter()
                .find(|(own, _)| own == label)
                .map_or(DialogAnswer::Refuse, |(_, answer)| *answer),
            MessageDialogResult::Yes => first.1,
            MessageDialogResult::No => second.1,
            _ => DialogAnswer::Refuse,
        }
    }
}
