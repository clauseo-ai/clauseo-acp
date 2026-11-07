//! Whether a label read from a path is our own Low label (N7), never to be recorded as the
//! lawyer's earlier label.

use super::Label;
use crate::windows::acl_plan::{Inherit, Rights};

/// Whether `label` is the Low label a [`Rights::LowLabel`] grant reaching `inherit` writes (on
/// a folder or a file): our own, never to be recorded as the lawyer's earlier label (N7).
pub(crate) fn is_our_label(label: &Label, inherit: Inherit) -> bool {
    /// `INHERITED_ACE`, which a label read may carry.
    const INHERITED: u8 = 0x10;
    /// `S-1-16-4096`'s level.
    const LOW: u32 = 4096;
    let ours: &[u8] = match inherit {
        Inherit::ThisOnly => &[0],
        // Object and container inherit on a folder; none on a file.
        Inherit::Tree => &[0, 0x3],
        // Object inherit, inherit only, no propagation.
        Inherit::FilesInFolder => &[0xd],
        // Object and container inherit, inherit only: what is below a folder.
        Inherit::Below => &[0xb],
    };
    label.level == LOW
        && label.mask == Rights::LowLabel.mask()
        && ours.contains(&(label.flags & !INHERITED))
}
