//! Tests of which entries of `.github/workflows` the pins check reads.

use super::*;
use crate::scratch::Scratch;

/// GitHub reads the `.yml` and `.yaml` files at the top of the folder. Any other entry is not
/// read, and a folder named like a workflow is not read as one.
#[test]
fn only_yaml_files_at_the_top_of_the_folder_are_read() {
    let dir = repo("workflow-files");
    put(
        &dir,
        ".github/workflows/ci.yml",
        "jobs:\n  a:\n    steps:\n      - run: echo ok\n",
    );
    put(
        &dir,
        ".github/workflows/notes.txt",
        "rustup toolchain install 3.4.5\n",
    );
    Scratch::create_dir_all(dir.join(".github/workflows/old.yml")).expect("create the folder");
    let found = check(&dir);
    assert!(found.violations.is_empty(), "{:?}", found.violations);
    // `Cargo.toml` from `repo`, and `ci.yml`: the notes and the folder are not counted.
    assert_eq!(found.inspected, 2);
}

/// A `.github/actions` that is a file, not a folder, fails at that path like the workflows
/// folder does.
#[test]
fn an_actions_path_that_is_not_a_folder_fails_at_the_path() {
    let dir = repo("actions-not-a-folder");
    put(&dir, ".github/actions", "not a folder\n");
    let found = check(&dir);
    let [violation] = found.violations.as_slice() else {
        panic!("one violation, at the path: {:?}", found.violations);
    };
    assert_eq!(violation.path, ".github/actions");
    assert!(
        violation
            .message
            .starts_with("could not read `.github/actions`: "),
        "{}",
        violation.message
    );
}

/// A `.github/workflows` that is a file, not a folder, cannot be listed: the check fails at
/// the folder rather than passing it.
#[test]
fn a_workflows_path_that_is_not_a_folder_fails_at_the_folder() {
    let dir = repo("workflows-not-a-folder");
    put(&dir, ".github/workflows", "not a folder\n");
    let found = check(&dir);
    let [violation] = found.violations.as_slice() else {
        panic!("one violation, at the folder: {:?}", found.violations);
    };
    assert_eq!(
        (violation.path.as_str(), violation.line),
        (".github/workflows", 1)
    );
    assert!(
        violation
            .message
            .starts_with("could not read `.github/workflows`: "),
        "{}",
        violation.message
    );
}
