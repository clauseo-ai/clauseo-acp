//! Tests of the module map: how a `mod` line resolves and what a Unix-only one carries.

use super::*;
use crate::checks::unix_paths::scan::scan;

/// The scans of `files`, each `(path, text)`.
fn scans(files: &[(&str, &str)]) -> BTreeMap<String, FileScan> {
    files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), scan(text, &["/opt/"]).expect("lexes")))
        .collect()
}

#[test]
fn a_mod_line_resolves_as_rustc_resolves_it() {
    assert_eq!(module_dir("crates/a/src/lib.rs"), "crates/a/src");
    assert_eq!(module_dir("crates/a/src/agent.rs"), "crates/a/src/agent");
    assert_eq!(
        module_dir("crates/a/src/agent/mod.rs"),
        "crates/a/src/agent"
    );
    assert_eq!(module_dir("crates/a/tests/stdio.rs"), "crates/a/tests");
    assert_eq!(
        module_dir("crates/a/tests/stdio/harness.rs"),
        "crates/a/tests/stdio/harness"
    );
    assert_eq!(module_dir("crates/a/src/bin/tool.rs"), "crates/a/src/bin");
}

#[test]
fn a_unix_mod_line_carries_down_and_a_plain_second_line_undoes_it() {
    let found = scans(&[
        (
            "crates/a/src/lib.rs",
            "#[cfg(unix)]\nmod u;\nmod both;\nmod outer { mod deep; }\n",
        ),
        ("crates/a/src/u.rs", "mod v;\n"),
        ("crates/a/src/u/v/mod.rs", ""),
        ("crates/a/src/both.rs", ""),
        ("crates/a/src/outer/deep.rs", ""),
        (
            "crates/a/tests/t.rs",
            "#[cfg(unix)]\n#[path = \"../src/both.rs\"]\nmod both;\n",
        ),
    ]);
    let unix: Vec<String> = unix_files(&found).into_iter().collect();
    assert_eq!(unix, ["crates/a/src/u.rs", "crates/a/src/u/v/mod.rs"]);
}
