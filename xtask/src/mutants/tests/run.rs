//! Tests of `run` in `mutants.rs`: its exit code, on a folder that is not a git checkout, so no
//! diff is read and no cargo-mutants runs.

use super::*;
use crate::scratch::Scratch;

#[test]
fn mutants_run_in_a_folder_that_is_no_git_checkout_fails_before_any_mutant_runs() {
    let root = Scratch::new("mutants-run");
    assert_eq!(run(&root), ExitCode::FAILURE);
}
