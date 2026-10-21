//! The builders of the tauri-config planted cases with a symlink to a folder in the desktop crate.

use super::cases::tree_case;
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// `src/lib.rs` without a call, and `src/x`, a symlink to the desktop crate's folder above it:
/// a walk that followed it would loop.
pub(super) fn symlink_to_a_folder_above() -> Repo {
    let repo = tree_case(
        "symlink-above",
        &[("crates/desktop/src/lib.rs", "pub fn run() {}\n")],
    );
    let link = repo.ctx().root.join("crates/desktop/src/x");
    Scratch::symlink_dir("..", link).expect("link the folder");
    repo
}
