//! Whether a label read from a path is our own Low label (N7), never to be recorded as the
//! lawyer's earlier label.

use super::file::{Slot, State};
use super::{EntryId, EntryState, Label};
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
        // Object inherit, inherit only: the files below a folder.
        Inherit::FilesBelow => &[0x9],
    };
    label.level == LOW
        && label.mask == Rights::LowLabel.mask()
        && ours.contains(&(label.flags & !INHERITED))
}

/// Records `label` as entry `id`'s prior label when it is still pending with none recorded,
/// and `label` isn't our own; whether it did.
pub(super) fn set_prior(state: &mut State, id: EntryId, label: Label) -> bool {
    let mut changed = false;
    for slot in &mut state.slots {
        if let Slot::Known(known) = slot
            && known.entry.id == id
            && known.entry.state == EntryState::Pending
            && known.entry.prior_label.is_none()
            && !is_our_label(&label, known.entry.inherit)
        {
            known.entry.prior_label = Some(label);
            changed = true;
        }
    }
    changed
}
