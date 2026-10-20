//! Tests of the `buildtime-edits` check: the planted cases are in `tests/planted.rs`, and the
//! builders that make their trees are here.

pub(super) mod planted;

use super::*;
use crate::checks::planted::Repo;
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

/// The scratch repo `repo(case, files)`, with the desktop crate at its usual place.
fn run_on<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)]) -> Repo {
    Repo::desktop(repo(case, files))
}

pub(super) fn clean_src_passes() -> Repo {
    run_on(
        "clean-src",
        &[
            (APP, "<main data-test=\"1\" />\n"),
            ("ui/app/src/views/V.tsx", "<div />\n"),
        ],
    )
}

pub(super) fn left_attribute() -> Repo {
    run_on("left", &[(APP, "<main data-buildtime=\"2\" />\n")])
}

pub(super) fn two_lines_in_one_file() -> Repo {
    run_on(
        "two-lines",
        &[(
            APP,
            "<main data-buildtime=\"1\">\n  <p>x</p>\n  <i data-buildtime=\"2\" />\n</main>\n",
        )],
    )
}

pub(super) fn nested_file() -> Repo {
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

pub(super) fn non_utf8_file() -> Repo {
    run_on(
        "non-utf8",
        &[(
            "ui/app/src/logo.bin",
            b"\xff\xfe data-buildtime=\"1\"".as_slice(),
        )],
    )
}

pub(super) fn single_quoted() -> Repo {
    run_on("single-quoted", &[(APP, "<main data-buildtime='2' />\n")])
}

pub(super) fn braced_value() -> Repo {
    run_on("braced", &[(APP, "<main data-buildtime={2} />\n")])
}

/// A formatter that puts each attribute on its own line moves the edit to line 3.
pub(super) fn split_tag() -> Repo {
    let text = "export function App() {\n  return <main\n    data-buildtime=\"2\"\n  />;\n}\n";
    run_on("split-tag", &[(APP, text)])
}

pub(super) fn space_before_equals() -> Repo {
    run_on(
        "space-before-equals",
        &[(APP, "<main data-buildtime = \"2\" />\n")],
    )
}

/// The name on line 2 and its `=` on line 3: the edit is at the name's line.
pub(super) fn name_and_value_split() -> Repo {
    let text = "<main\n  data-buildtime\n  =\"2\"\n/>\n";
    run_on("name-value-split", &[(APP, text)])
}

pub(super) fn crlf_file() -> Repo {
    run_on(
        "crlf",
        &[(APP, "<main>\r\n<p data-buildtime=\"2\" />\r\n</main>\r\n")],
    )
}

pub(super) fn stylesheet() -> Repo {
    let css = "main {\n  color: black;\n}\n[data-buildtime=\"2\"] {\n  outline: 0;\n}\n";
    run_on(
        "stylesheet",
        &[(APP, "<main />\n"), ("ui/app/src/app.css", css)],
    )
}

pub(super) fn hidden_deep_folder() -> Repo {
    run_on(
        "hidden-deep",
        &[
            (APP, "<main />\n"),
            (
                "ui/app/src/.cache/a/b/state.json",
                "{\"html\": \"data-buildtime=1\"}\n",
            ),
        ],
    )
}

/// Names that only start like the attribute, or the name with no `=` after it, pass.
pub(super) fn lookalikes_pass() -> Repo {
    let text = "<main data-buildtime-ms=\"2\" data-buildtimes=\"3\" />\n// data-buildtime is set by the probe\n";
    run_on("lookalikes", &[(APP, text)])
}

/// A file with the attribute while another process holds the buildtime lock: a live run.
pub(super) fn live_run_holds_the_edit() -> Repo {
    let dir = repo("live", &[(APP, "<main data-buildtime=\"2\" />\n")]);
    let held = lock::acquire(&dir).expect("take the lock");
    Repo::desktop(dir).holding(held)
}

pub(super) fn missing_src() -> Repo {
    run_on::<&str>("missing-src", &[])
}

pub(super) fn empty_src() -> Repo {
    let dir = Scratch::new("planted-buildtime-edits-empty-src");
    Scratch::create_dir_all(dir.join(SRC)).expect("create the src dir");
    Repo::desktop(dir)
}

#[test]
fn a_clean_tree_never_creates_the_lock_file() {
    let dir = repo("no-lock", &[(APP, "<main />\n")]);
    run(&desktop_context(&dir));
    assert!(!dir.join(lock::LOCK_PATH).exists());
}
