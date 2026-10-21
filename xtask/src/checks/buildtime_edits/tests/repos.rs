//! The builders of the buildtime-edits planted cases: each a scratch repo with the desktop crate as
//! its one member, holding the UI sources the case needs.

use super::APP;
use crate::buildtime::lock;
use crate::checks::buildtime_edits::SRC;
use crate::checks::planted::{Repo, scratch};
use crate::scratch::Scratch;

pub(super) fn clean_src_passes() -> Repo {
    Repo::desktop(scratch(
        "buildtime-edits-clean-src",
        &[
            (APP, "<main data-test=\"1\" />\n"),
            ("ui/app/src/views/V.tsx", "<div />\n"),
        ],
    ))
}

pub(super) fn left_attribute() -> Repo {
    Repo::desktop(scratch(
        "buildtime-edits-left",
        &[(APP, "<main data-buildtime=\"2\" />\n")],
    ))
}

pub(super) fn two_lines_in_one_file() -> Repo {
    let text = "<main data-buildtime=\"1\">\n  <p>x</p>\n  <i data-buildtime=\"2\" />\n</main>\n";
    Repo::desktop(scratch("buildtime-edits-two-lines", &[(APP, text)]))
}

pub(super) fn nested_file() -> Repo {
    Repo::desktop(scratch(
        "buildtime-edits-nested",
        &[
            (APP, "<main>\n</main>\n"),
            (
                "ui/app/src/views/V.tsx",
                "export function V() {\n  return <div data-buildtime=\"3\">x</div>;\n}\n",
            ),
        ],
    ))
}

pub(super) fn non_utf8_file() -> Repo {
    Repo::desktop(scratch(
        "buildtime-edits-non-utf8",
        &[(
            "ui/app/src/logo.bin",
            b"\xff\xfe data-buildtime=\"1\"".as_slice(),
        )],
    ))
}

pub(super) fn single_quoted() -> Repo {
    let text = "<main data-buildtime='2' />\n";
    Repo::desktop(scratch("buildtime-edits-single-quoted", &[(APP, text)]))
}

pub(super) fn braced_value() -> Repo {
    let text = "<main data-buildtime={2} />\n";
    Repo::desktop(scratch("buildtime-edits-braced", &[(APP, text)]))
}

/// A formatter that puts each attribute on its own line moves the edit to line 3.
pub(super) fn split_tag() -> Repo {
    let text = "export function App() {\n  return <main\n    data-buildtime=\"2\"\n  />;\n}\n";
    Repo::desktop(scratch("buildtime-edits-split-tag", &[(APP, text)]))
}

pub(super) fn space_before_equals() -> Repo {
    let text = "<main data-buildtime = \"2\" />\n";
    Repo::desktop(scratch(
        "buildtime-edits-space-before-equals",
        &[(APP, text)],
    ))
}

/// The name on line 2 and its `=` on line 3: the edit is at the name's line.
pub(super) fn name_and_value_split() -> Repo {
    let text = "<main\n  data-buildtime\n  =\"2\"\n/>\n";
    Repo::desktop(scratch("buildtime-edits-name-value-split", &[(APP, text)]))
}

pub(super) fn crlf_file() -> Repo {
    let text = "<main>\r\n<p data-buildtime=\"2\" />\r\n</main>\r\n";
    Repo::desktop(scratch("buildtime-edits-crlf", &[(APP, text)]))
}

pub(super) fn stylesheet() -> Repo {
    let css = "main {\n  color: black;\n}\n[data-buildtime=\"2\"] {\n  outline: 0;\n}\n";
    Repo::desktop(scratch(
        "buildtime-edits-stylesheet",
        &[(APP, "<main />\n"), ("ui/app/src/app.css", css)],
    ))
}

pub(super) fn hidden_deep_folder() -> Repo {
    Repo::desktop(scratch(
        "buildtime-edits-hidden-deep",
        &[
            (APP, "<main />\n"),
            (
                "ui/app/src/.cache/a/b/state.json",
                "{\"html\": \"data-buildtime=1\"}\n",
            ),
        ],
    ))
}

/// Names that only start like the attribute, or the name with no `=` after it, pass.
pub(super) fn lookalikes_pass() -> Repo {
    let text = "<main data-buildtime-ms=\"2\" data-buildtimes=\"3\" />\n// data-buildtime is set by the probe\n";
    Repo::desktop(scratch("buildtime-edits-lookalikes", &[(APP, text)]))
}

/// A file with the attribute while another process holds the buildtime lock: a live run.
pub(super) fn live_run_holds_the_edit() -> Repo {
    let dir = scratch(
        "buildtime-edits-live",
        &[(APP, "<main data-buildtime=\"2\" />\n")],
    );
    let held = lock::acquire(&dir).expect("take the lock");
    Repo::desktop(dir).holding(held)
}

pub(super) fn missing_src() -> Repo {
    Repo::desktop(scratch::<&str>("buildtime-edits-missing-src", &[]))
}

pub(super) fn empty_src() -> Repo {
    let dir = scratch::<&str>("buildtime-edits-empty-src", &[]);
    Scratch::create_dir_all(dir.join(SRC)).expect("create the src dir");
    Repo::desktop(dir)
}
