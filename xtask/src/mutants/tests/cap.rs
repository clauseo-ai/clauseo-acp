//! Tests of a run stopped at the cap: the fix, the missed mutants it had found, and the build
//! dirs it left.

use super::*;

#[test]
fn a_run_past_the_cap_fails_with_split_the_change() {
    let (printed, passed, _) = check(
        "mutants-cap",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             `{CMD}` did not finish within 20 min, so it was stopped\n    fix: split the change: cargo xtask mutants caps a run at 20 minutes\n"
        )
    );
}

#[test]
fn a_run_past_the_cap_lists_the_mutants_it_had_missed() {
    let outcomes = lab(
        &[
            baseline("Success"),
            mutant("MissedMutant", 106, 5, "replace double -> u32 with 0"),
            mutant(
                "CaughtMutant",
                110,
                5,
                "replace main -> ExitCode with Default::default()",
            ),
        ],
        false,
    );
    let (printed, passed, _) = check(
        "mutants-cap-partial",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::WritesThenTimesOut(outcomes),
    );
    assert!(!passed);
    // The scratch dir's name ends in a counter (`Scratch::new`), so the last line is matched by
    // its start and end around it.
    let (printed, removed) = printed
        .trim_end()
        .rsplit_once('\n')
        .expect("the run prints a line after the summary");
    let scratch =
        std::env::temp_dir().join(format!("xtask-mutants-cap-partial-{}-", std::process::id()));
    let removed_start = format!(
        "removed 1 build dir cargo-mutants left in {}",
        scratch.display()
    );
    assert!(
        removed.starts_with(&removed_start)
            && removed.ends_with(&format!(
                "{}tmp when it was stopped",
                std::path::MAIN_SEPARATOR
            )),
        "{removed}"
    );
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             `{CMD}` did not finish within 20 min, so it was stopped\n    fix: split the change: \
             cargo xtask mutants caps a run at 20 minutes\n\
             xtask/src/main.rs:106: missed mutant: no test fails on `replace double -> u32 with \
             0`\n{MISSED_FIX}\n\
             mutants FAILED at the cap, after 2 mutants tested: 1 missed, 1 caught, 0 timed out, \
             0 unviable"
        )
    );
}

/// A build dir the stopped run left that cannot be removed: the failure names it and the OS
/// error, on one line, with the fix.
#[cfg(unix)]
#[test]
fn a_left_build_dir_that_cannot_be_removed_fails_with_its_path_and_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = Scratch::new("mutants-stuck-build-dir");
    let temp = root.join("tmp");
    let build = temp.join(format!("{}stuck.tmp", build_dir_prefix(&root)));
    Scratch::create_dir_all(build.join("target")).expect("make a build dir");
    // A read-only build dir keeps its contents, so remove_dir_all fails.
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    Scratch::set_permissions(&build, mode(0o555)).expect("make it read-only");
    let mut out = Printer::new(Vec::new(), false);
    remove_left_build_dirs(&mut out, &temp, &root, &BTreeSet::new());
    Scratch::set_permissions(&build, mode(0o755)).expect("make it writable again");
    let printed = String::from_utf8(out.into_inner()).expect("utf-8");
    assert_eq!(
        printed,
        format!(
            "could not remove {}, a build dir cargo-mutants left when it was stopped: \
             Permission denied (os error 13)\n    fix: delete that folder by hand; nothing else \
             uses it\n",
            build.display()
        )
    );
}
