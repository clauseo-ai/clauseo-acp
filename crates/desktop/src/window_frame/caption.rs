//! The native maximise button's pointer states (Windows, `windows.rs`) as a pure function: what
//! each mouse message over the native window does to the page's look and to the window.
//!
//! The native window answers `HTMAXBUTTON`, so the pointer reaches it as non-client messages
//! (`WM_NCMOUSEMOVE`, `WM_NCLBUTTONDOWN`) until a press captures the mouse; from then on until
//! the release it gets client messages (`WM_MOUSEMOVE`, `WM_LBUTTONUP`). A click is a press and
//! a release both over the button, as Windows' own caption buttons count one.

/// Where the pointer stands with the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Pointer {
    /// Not over the button, or over it after a click until it moves again.
    #[default]
    Away,
    /// Over the button, no button pressed.
    Hover,
    /// Pressed on the button and still over it; the mouse is captured.
    Press,
    /// Pressed on the button, then moved off it with the button still down; captured.
    PressOff,
}

/// A mouse message over the native window, already sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    /// `WM_NCMOUSEMOVE`: the pointer moves over the button, nothing captured.
    Move,
    /// `WM_NCMOUSELEAVE`: the pointer left the button.
    Leave,
    /// `WM_NCLBUTTONDOWN` or `WM_NCLBUTTONDBLCLK` on the button.
    Down,
    /// `WM_MOUSEMOVE` while captured; `over` is whether the pointer is over the button.
    Drag { over: bool },
    /// `WM_LBUTTONUP` while captured; `over` as for `Drag`.
    Up { over: bool },
    /// `WM_CAPTURECHANGED`: the capture ended, by the release or by someone else taking it.
    CaptureLost,
}

/// The look the page draws on its maximise button: the value of its `data-pointer` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// No attribute: the page's own `:hover` and `:active` apply (which never fire while the
    /// native window covers the button).
    Rest,
    /// `data-pointer="hover"`.
    Hover,
    /// `data-pointer="press"`.
    Press,
}

impl Look {
    /// The script that puts this look on the page's `[data-caption="maximise"]`. Fixed strings:
    /// nothing from anywhere is put into them.
    pub(crate) const fn script(self) -> &'static str {
        match self {
            Self::Rest => {
                "document.querySelector('[data-caption=\"maximise\"]')\
                 ?.removeAttribute('data-pointer')"
            }
            Self::Hover => {
                "document.querySelector('[data-caption=\"maximise\"]')\
                 ?.setAttribute('data-pointer','hover')"
            }
            Self::Press => {
                "document.querySelector('[data-caption=\"maximise\"]')\
                 ?.setAttribute('data-pointer','press')"
            }
        }
    }
}

/// What to do with the mouse capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Capture {
    /// Nothing.
    #[default]
    Keep,
    /// Capture the mouse (`SetCapture`).
    Take,
    /// Release it (`ReleaseCapture`). Done after the state is stored, since it sends
    /// `WM_CAPTURECHANGED` back at once.
    Release,
}

/// What to do for one input; every field is independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Step {
    /// The state after the input.
    pub(crate) next: Pointer,
    /// The page's new look, when it changes.
    pub(crate) look: Option<Look>,
    /// Ask for `WM_NCMOUSELEAVE` (`TrackMouseEvent` with `TME_LEAVE | TME_NONCLIENT`).
    pub(crate) track_leave: bool,
    /// What to do with the mouse capture.
    pub(crate) capture: Capture,
    /// Maximise the window, or restore it when it is maximised: the click.
    pub(crate) toggle: bool,
}

/// What `input` does in state `now`.
pub(crate) fn step(now: Pointer, input: Input) -> Step {
    let stay = Step {
        next: now,
        ..Step::default()
    };
    let to = |next: Pointer, look: Look| Step {
        next,
        look: Some(look),
        ..Step::default()
    };
    match (now, input) {
        (Pointer::Away, Input::Move) => Step {
            track_leave: true,
            ..to(Pointer::Hover, Look::Hover)
        },
        (Pointer::Away | Pointer::Hover, Input::Down) => Step {
            capture: Capture::Take,
            ..to(Pointer::Press, Look::Press)
        },
        (Pointer::Hover, Input::Leave)
        | (Pointer::Press | Pointer::PressOff, Input::CaptureLost) => to(Pointer::Away, Look::Rest),
        (Pointer::Press, Input::Drag { over: false }) => to(Pointer::PressOff, Look::Rest),
        (Pointer::PressOff, Input::Drag { over: true }) => to(Pointer::Press, Look::Press),
        // The button moves when the window maximises or restores, so the click leaves it at
        // rest; the next move over it brings the hover back.
        (Pointer::Press | Pointer::PressOff, Input::Up { over }) => Step {
            capture: Capture::Release,
            toggle: over,
            ..to(Pointer::Away, Look::Rest)
        },
        // Already hovering; a leave while pressed (the capture moves the pointer's owner);
        // client messages with no capture; a second press while captured.
        _ => stay,
    }
}

/// The point a mouse message's `lparam` carries: x in the low 16 bits, y in the next 16, each
/// signed (`GET_X_LPARAM`, `GET_Y_LPARAM`), so a captured pointer left of or above the window
/// comes out negative.
pub(crate) fn lparam_point(lparam: isize) -> (i32, i32) {
    let signed = |word: isize| {
        let word = i32::try_from(word & 0xFFFF).unwrap_or(0);
        if word >= 0x8000 {
            word - 0x1_0000
        } else {
            word
        }
    };
    (signed(lparam), signed(lparam >> 16))
}

/// A system-menu item's text as a name: up to any tab (the shortcut after it), without the
/// accelerator: a `&` before a letter goes, `&&` is a plain `&`, and a `(&X)` as East Asian
/// menus write it goes whole. So "Ma&ximize" names "Maximize", and "最大化(&X)" "最大化".
pub(crate) fn menu_word(item: &str) -> String {
    let item = item.split('\t').next().unwrap_or_default();
    let chars: Vec<char> = item.chars().collect();
    let mut word = String::with_capacity(item.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        if c == '(' && next == Some('&') && chars.get(at + 3) == Some(&')') {
            at += 4;
        } else if c == '&' && next == Some('&') {
            word.push('&');
            at += 2;
        } else {
            if c != '&' {
                word.push(c);
            }
            at += 1;
        }
    }
    word.trim().to_owned()
}

#[cfg(test)]
mod tests;
