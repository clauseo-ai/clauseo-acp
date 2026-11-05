//! What the walk decides about one child before it touches it (S15 `PR3b` round 2): whether its
//! name is one the set-aside protects (N1), which writes it needs and in what order (N13), and
//! which of them its handle has no right for (N3). Pure, so it is tested on every OS.

/// The names the set-aside protects at any depth below a matter (N1).
pub(crate) const PROTECTED_NAMES: [&str; 3] = [".git", ".claude", ".mcp.json"];

/// Whether `name`, one path part as UTF-16, is one of [`PROTECTED_NAMES`], ignoring ASCII case
/// (NTFS folds the case of these names for every lookup).
pub(crate) fn protected_name(name: &[u16]) -> bool {
    PROTECTED_NAMES.iter().any(|protected| {
        protected.len() == name.len()
            && protected
                .bytes()
                .zip(name)
                .all(|(want, &got)| u16::from(want) == ascii_lower(got))
    })
}

/// `unit` with an ASCII capital lowered; anything else as it is.
fn ascii_lower(unit: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
        unit + 32
    } else {
        unit
    }
}

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

/// The writes a child needs, in the order they are made: the DACL, then the owner, then the
/// label. The DACL goes before the owner so a hand-back that fails between the two leaves a
/// file a pool user still owns, which the next hand-back takes again; the other order would
/// leave a file the lawyer owns that still carries the agent's broad allows, which no hand-back
/// would touch again, since no pool user owns it (N13).
pub(crate) fn writes(dacl: bool, owner: bool, label: bool) -> Vec<Write> {
    [
        (dacl, Write::Dacl),
        (owner, Write::Owner),
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
