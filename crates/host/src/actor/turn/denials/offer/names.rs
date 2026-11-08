//! The two name rules the offer layer asks by name. Stage C exports them from `clauseo_sandbox`
//! (#735, from its `ascii_fold.rs`), and the host's #766 carries the one vendored copy in
//! `crate::network_paths`. [`name_folds_to`] is that copy's; [`refused_in_other_matter`] is
//! answered here with it until `network_paths` exports its own (it is private to
//! `network_paths::ascii_fold` at #766's f6f91e9e), so the switch is an import change.

use std::ffi::OsStr;

pub(crate) use crate::network_paths::name_folds_to;

/// The folders never offered in another matter: its history, Clauseo's and the agents' own
/// (the identity layer captures each matter's). `network_paths::ascii_fold`'s list.
pub(crate) const REFUSED_IN_OTHER_MATTER_NAMES: [&str; 5] =
    [".git", ".clauseo", ".claude", ".codex", ".agents"];

/// Whether `name` is one of the folders never offered in another matter (`.git`, `.clauseo`,
/// `.claude`, `.codex`, `.agents`), as a case-insensitive volume opens it ([`name_folds_to`]).
pub(crate) fn refused_in_other_matter(name: &OsStr) -> bool {
    REFUSED_IN_OTHER_MATTER_NAMES
        .iter()
        .any(|refused| name_folds_to(name, refused))
}
