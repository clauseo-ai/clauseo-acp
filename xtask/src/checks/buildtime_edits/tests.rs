//! Tests of the `buildtime-edits` check: the planted cases are in `tests/planted.rs`, and the
//! builders that make their trees are here.

pub(super) mod planted;

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;

/// The UI source the probe edits, in the planted trees.
const APP: &str = "ui/app/src/App.tsx";

/// A scratch repo named after `case`, with each `(path, contents)` of `files` written into it.
fn repo<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)]) -> Scratch {
    let dir = Scratch::new(&format!("planted-buildtime-edits-{case}"));
    for (path, contents) in files {
        let full = dir.join(path);
        Scratch::create_dir_all(full.parent().expect("a file has a parent"))
            .expect("create the dirs");
        Scratch::write(full, contents).expect("write the file");
    }
    dir
}

/// Runs the check on the scratch repo `repo(case, files)`.
fn run_on<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)]) -> Inspection {
    run(&desktop_context(&repo(case, files)))
}

pub(super) fn clean_src_passes() -> Inspection {
    run_on(
        "clean-src",
        &[
            (APP, "<main data-test=\"1\" />\n"),
            ("ui/app/src/views/V.tsx", "<div />\n"),
        ],
    )
}

pub(super) fn left_attribute() -> Inspection {
    run_on("left", &[(APP, "<main data-buildtime=\"2\" />\n")])
}

pub(super) fn two_lines_in_one_file() -> Inspection {
    run_on(
        "two-lines",
        &[(
            APP,
            "<main data-buildtime=\"1\">\n  <p>x</p>\n  <i data-buildtime=\"2\" />\n</main>\n",
        )],
    )
}

pub(super) fn nested_file() -> Inspection {
    run_on(
        "nested",
        &[
            (APP, "<main>\n</main>\n"),
            (
                "ui/app/src/views/V.tsx",
                "export function V() {\n  return <div data-buildtime=\"3\">x</div>;\n}\n",
            ),
        ],
    )
}

pub(super) fn non_utf8_file() -> Inspection {
    run_on(
        "non-utf8",
        &[(
            "ui/app/src/logo.bin",
            b"\xff\xfe data-buildtime=\"1\"".as_slice(),
        )],
    )
}

/// A file with the attribute while another process holds the buildtime lock: a live run.
pub(super) fn live_run_holds_the_edit() -> Inspection {
    let dir = repo("live", &[(APP, "<main data-buildtime=\"2\" />\n")]);
    let held = lock::acquire(&dir.join(lock::LOCK_PATH)).expect("take the lock");
    let found = run(&desktop_context(&dir));
    drop(held);
    found
}

pub(super) fn missing_src() -> Inspection {
    run_on::<&str>("missing-src", &[])
}

pub(super) fn empty_src() -> Inspection {
    let dir = Scratch::new("planted-buildtime-edits-empty-src");
    Scratch::create_dir_all(dir.join(SRC)).expect("create the src dir");
    run(&desktop_context(&dir))
}

#[test]
fn a_clean_tree_never_creates_the_lock_file() {
    let dir = repo("no-lock", &[(APP, "<main />\n")]);
    run(&desktop_context(&dir));
    assert!(!dir.join(lock::LOCK_PATH).exists());
}
