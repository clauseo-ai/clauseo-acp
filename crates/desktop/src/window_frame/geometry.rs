//! The frame's geometry as pure functions: where the page draws its maximise button (Windows),
//! and where the native traffic lights go (Mac). Both follow the toolbar's tokens in
//! `ui/components/src/styles/tokens-parts.css` and `ui/components/src/clauseo/shell.css`;
//! `tests.rs` reads those files and fails when they disagree.

/// A caption button's width, in CSS px: `--size-caption-w`. Windows 11's own metric, which does
/// not follow the text size.
#[cfg(any(windows, test))]
pub(crate) const CAPTION_W: f64 = 46.0;
/// A caption button's height, in CSS px: `--size-caption-h`.
#[cfg(any(windows, test))]
pub(crate) const CAPTION_H: f64 = 32.0;

/// A rectangle in physical px of the window's client area: `left` and `top` inside, `right` and
/// `bottom` outside, as Win32's `RECT`.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[cfg(any(windows, test))]
impl Rect {
    /// Whether the point (`x`, `y`) is inside.
    pub(crate) const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// The width.
    pub(crate) const fn width(&self) -> i32 {
        self.right - self.left
    }

    /// The height.
    pub(crate) const fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// CSS px times `scale`, rounded to a physical pixel as the webview lays the edge out.
#[cfg(any(windows, test))]
#[expect(
    clippy::cast_possible_truncation,
    reason = "a window edge in physical px, far inside i32"
)]
fn physical(css: f64, scale: f64) -> i32 {
    (css * scale).round() as i32
}

/// The page's maximise button in a client area `client_width` physical px wide at `scale`
/// physical px per CSS px: the middle of the three caption buttons, flush with the top right
/// corner (close is the last, at the corner). Empty when the window is narrower than the buttons.
#[cfg(any(windows, test))]
pub(crate) fn maximise_button(client_width: i32, scale: f64) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let right = client_width - physical(CAPTION_W, scale);
    let left = client_width - physical(2.0 * CAPTION_W, scale);
    if left < 0 {
        return Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
    }
    Rect {
        left,
        top: 0,
        right,
        bottom: physical(CAPTION_H, scale),
    }
}

/// Whether a client point (`x`, `y`, physical px) is over the page's maximise button.
#[cfg(test)]
pub(crate) fn over_maximise(x: i32, y: i32, client_width: i32, scale: f64) -> bool {
    maximise_button(client_width, scale).contains(x, y)
}

/// The toolbar's lead starts its corner lights this far from the window's left edge, in CSS px:
/// `.room-lead`'s `padding-left`, `--s5`.
#[cfg(any(target_os = "macos", test))]
pub(crate) const LEAD_PAD: f64 = 20.0;
/// The lead's height, in CSS px: `--lead-h`. The lights are centred in it.
#[cfg(any(target_os = "macos", test))]
pub(crate) const LEAD_H: f64 = 52.0;
/// A corner light's diameter, in CSS px: `--s3`.
#[cfg(any(target_os = "macos", test))]
pub(crate) const LIGHT: f64 = 12.0;
/// From one corner light's left edge to the next, in CSS px: `--s3` plus the gap `--s2`. The
/// native lights stand 23 pt apart on macOS 27, so only the first native light lands on its
/// corner light; the other two stand 3 and 6 pt to the right of theirs.
#[cfg(test)]
pub(crate) const LIGHT_PITCH: f64 = 20.0;
/// The native close button's frame's width, in points: the light it draws fills it, 14 pt on
/// macOS 27 (measured), wider than the design's 12 px.
#[cfg(any(target_os = "macos", test))]
pub(crate) const NATIVE_BUTTON_W: f64 = 14.0;
/// Its height, in points.
#[cfg(any(target_os = "macos", test))]
pub(crate) const NATIVE_BUTTON_H: f64 = 16.0;
/// From one native light's left edge to the next, in points (macOS 27, measured): the box the
/// page keeps for them, `--lights-native-w`, follows from it.
#[cfg(test)]
pub(crate) const NATIVE_PITCH: f64 = 23.0;
/// How far the native close button's frame stands above the bottom of its title bar, in points
/// (macOS 27, measured). The inset (`inset_traffic_lights` in wry and tao) keeps it: it sets the
/// title bar's height to the button's height plus `y`, so the button's top lands `y` minus this
/// below the window's top.
#[cfg(any(target_os = "macos", test))]
pub(crate) const NATIVE_BUTTON_LIFT: f64 = 10.0;

/// The position for `traffic_light_position`: the close button's frame placed so its light
/// stands on the toolbar's first corner light (centred on it), centred in the lead's height.
#[cfg(any(target_os = "macos", test))]
pub(crate) const fn traffic_lights() -> (f64, f64) {
    let x = LEAD_PAD - (NATIVE_BUTTON_W - LIGHT) / 2.0;
    let y = (LEAD_H - NATIVE_BUTTON_H) / 2.0 + NATIVE_BUTTON_LIFT;
    (x, y)
}
