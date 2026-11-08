//! The lawyer's answer from what a dialog returned, as pure functions: the Windows task dialog's
//! (a button and the checkbox), and rfd's three buttons (macOS and Linux). Anything but a press
//! of an allowing button refuses, and Escape or the close button refuses whatever the checkbox
//! says.

use clauseo_host::DialogAnswer;
#[cfg(any(not(windows), test))]
use rfd::{MessageButtons, MessageDialogResult};

/// The task dialog's Allow button.
#[cfg(any(windows, test))]
pub(super) const ALLOW: i32 = 100;
/// Its Don't allow button, the default.
#[cfg(any(windows, test))]
pub(super) const REFUSE: i32 = 101;

/// The answer for the task dialog's `pressed` button with the checkbox `remember`ed: Allow
/// remembered is for the matter, Allow alone once, Don't allow remembered refuses for the
/// matter. Anything else (`IDCANCEL` for Escape, Alt+F4 and the close button, or no button)
/// refuses, checked or not.
#[cfg(any(windows, test))]
pub(super) fn from_task_dialog(pressed: i32, remember: bool) -> DialogAnswer {
    match (pressed, remember) {
        (ALLOW, true) => DialogAnswer::AllowForMatter,
        (ALLOW, false) => DialogAnswer::AllowOnce,
        (REFUSE, true) => DialogAnswer::RefuseForMatter,
        _ => DialogAnswer::Refuse,
    }
}

/// rfd's three buttons, each with its answer, in the order drawn.
#[cfg(any(not(windows), test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThreeButtons([(String, DialogAnswer); 3]);

/// The order of the three buttons on macOS (`true`) or Linux (`false`).
///
/// - macOS: `NSAlert`'s first button is its default (Return), so Don't allow comes first.
/// - Linux: rfd's gtk dialog awaited answers the plain `Yes`, `No` and `Cancel` of the three
///   positions, and `Cancel` is also Escape and the close button, so Don't allow comes third.
#[cfg(any(not(windows), test))]
pub(super) fn order(macos: bool) -> [DialogAnswer; 3] {
    if macos {
        [
            DialogAnswer::Refuse,
            DialogAnswer::AllowOnce,
            DialogAnswer::AllowForMatter,
        ]
    } else {
        [
            DialogAnswer::AllowOnce,
            DialogAnswer::AllowForMatter,
            DialogAnswer::Refuse,
        ]
    }
}

#[cfg(any(not(windows), test))]
impl ThreeButtons {
    /// The buttons in `order`, labelled `once`, `always` and `refuse`.
    pub(super) fn new(order: [DialogAnswer; 3], once: &str, always: &str, refuse: &str) -> Self {
        Self(order.map(|answer| {
            let label = match answer {
                DialogAnswer::AllowOnce => once,
                DialogAnswer::AllowForMatter => always,
                DialogAnswer::Refuse | DialogAnswer::RefuseForMatter => refuse,
            };
            (label.to_owned(), answer)
        }))
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
