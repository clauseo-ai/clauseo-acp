//! What the walk decides about one child before it touches it (S15 `PR3b` round 2): which writes
//! it needs and in what order (N13), and which of them its handle has no right for (N3). Pure, so
//! it is tested on every OS.

/// One write on a child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Write {
    /// Its DACL: inherited entries worked out again, cleaned when repaired or handed back.
    Dacl,
    /// Its owner: the hand-back's.
    Owner,
    /// Its mandatory label.
    Label,
}

impl Write {
    /// Whether it needs `WRITE_OWNER` (an owner or a label) rather than `WRITE_DAC`.
    fn needs_write_owner(self) -> bool {
        matches!(self, Write::Owner | Write::Label)
    }
}

/// The writes a child needs, in the order they are made: the owner, then the DACL, then the
/// label (S15 `PR4h`). The owner goes first, so once the cleaned DACL strips a pool user's
/// allows, that user owns the child no more and can't put one back through an owner's implied
/// `WRITE_DAC`; the hand-back is read again after (`handed`). A failure between the two leaves a
/// child the lawyer owns with the agent's allows still on it: the next strict walk refuses it as
/// [`BroadAllowChild`](crate::windows::writer::ApplyFailure::BroadAllowChild), fail closed. (N13
/// wrote the DACL first, which left the pool user owning the child in between.)
pub(crate) fn writes(dacl: bool, owner: bool, label: bool) -> Vec<Write> {
    [
        (owner, Write::Owner),
        (dacl, Write::Dacl),
        (label, Write::Label),
    ]
    .into_iter()
    .filter_map(|(needed, write)| needed.then_some(write))
    .collect()
}

/// The first of `writes` that a handle opened with `write_dac` and `write_owner` can't make.
/// Checked before any write, so a child is never left half done (N3).
pub(crate) fn lacking(writes: &[Write], write_dac: bool, write_owner: bool) -> Option<Write> {
    writes.iter().copied().find(|write| {
        if write.needs_write_owner() {
            !write_owner
        } else {
            !write_dac
        }
    })
}
