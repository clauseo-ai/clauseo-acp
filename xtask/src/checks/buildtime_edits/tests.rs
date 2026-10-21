//! Tests of the `buildtime-edits` check: the planted cases are in `tests/planted.rs`, and the
//! builders that make their trees are in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;
use crate::checks::planted::{failure, scratch};
use crate::checks::tests::desktop_context;

/// The UI source the probe edits, in the planted trees.
const APP: &str = "ui/app/src/App.tsx";

/// The zero-inspected failure of the buildtime-edits check, as it prints.
fn nothing_inspected() -> String {
    failure(
        "0 UI source files inspected under ui/app/src",
        crate::checks::Fix::NothingInspected {
            name: "buildtime-edits",
            many: "UI source files",
            place: "under ui/app/src".to_owned(),
            source: "xtask/src/checks/buildtime_edits.rs".to_owned(),
        },
    )
}

#[test]
fn a_clean_tree_never_creates_the_lock_file() {
    let dir = scratch("buildtime-edits-no-lock", &[(APP, "<main />\n")]);
    run(&desktop_context(&dir));
    assert!(!dir.join(lock::LOCK_PATH).exists());
}
