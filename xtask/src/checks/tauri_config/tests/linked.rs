//! The builders of the tauri-config planted cases with a symlink to a folder in the desktop crate.

use super::repos::tree_case;
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

/// `src/gen.rs`, a symlink to `elsewhere/ctx.rs`, a file outside the crate whose
/// `generate_context!("other.json")` names another config: the walk reads it through the link. On
/// Windows, where a scratch file symlink needs a privilege, it is a plain copy.
pub(super) fn symlink_to_a_rust_file() -> Repo {
    let call = "tauri::generate_context!(\"other.json\");\n";
    let repo = tree_case(
        "symlink-file",
        &[
            ("crates/desktop/src/lib.rs", "pub fn run() {}\n"),
            ("elsewhere/ctx.rs", call),
        ],
    );
    let link = repo.ctx().root.join("crates/desktop/src/gen.rs");
    #[cfg(unix)]
    Scratch::symlink("../../../elsewhere/ctx.rs", link).expect("link the file");
    #[cfg(not(unix))]
    Scratch::write(link, call).expect("write the file");
    repo
}
