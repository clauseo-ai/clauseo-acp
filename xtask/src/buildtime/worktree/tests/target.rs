//! Tests of the checks of the checkout's `target/` before the warm-up: where `cargo metadata`
//! says cargo builds, what `target/debug` is, and whether `target/xtask-run` is there. Each
//! refusal stops the run before the warm-up, so nothing is built or made.

use super::super::*;
use super::{Fake, find, measure_with, metadata_naming, repo};
use crate::scratch::Scratch;

/// `cargo metadata`'s command line, as failures name it.
const METADATA: &str = "cargo metadata --format-version 1 --no-deps";

/// Measures one run with `cargo metadata` printing `metadata`, and checks that a refusal
/// stopped it before the warm-up and before anything was made.
fn refused_with(root: &Path, metadata: Option<String>) -> Failure {
    let mut fake = Fake {
        metadata,
        ..Fake::default()
    };
    let refused = measure_with(root, false, 1, &mut fake).expect_err("refused");
    assert_eq!(fake.ran("cargo build"), 0, "no warm-up");
    assert_eq!(fake.ran("git worktree add"), 0, "nothing made");
    refused
}

#[test]
fn a_target_or_build_dir_other_than_the_checkouts_own_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-shared-target");
    let root = repo(&scratch);
    let own = root.join("target");
    let shared = scratch.join("shared/target");
    let elsewhere = root.join("out");
    for (target, build, field, named) in [
        (&shared, &own, "target_directory", &shared),
        (&own, &shared, "build_directory", &shared),
        (&elsewhere, &own, "target_directory", &elsewhere),
    ] {
        let refused = refused_with(&root, Some(metadata_naming(target, build)));
        assert_eq!(
            refused,
            Failure::new(
                format!(
                    "refusing to measure: cargo builds into {}, not {} (`{field}` of \
                     `{METADATA}`), so the warm-up and the worktree's first build would share it \
                     and the first build would reuse its output",
                    named.display(),
                    own.display()
                ),
                SHARED_TARGET_FIX
            )
        );
    }
    assert_eq!(
        SHARED_TARGET_FIX,
        "build into the checkout's own target/ for this run: unset `CARGO_TARGET_DIR`, \
         `CARGO_BUILD_TARGET_DIR` and `CARGO_BUILD_BUILD_DIR`, and take `build.target-dir` and \
         `build.build-dir` out of the cargo config files read here (`.cargo/config.toml` in the \
         checkout or a folder above it, and `config.toml` in `CARGO_HOME`); then run again"
    );
}

#[test]
fn metadata_without_a_dir_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-no-target-field");
    let root = repo(&scratch);
    let only_target = serde_json::json!({ "target_directory": root.join("target") });
    for (printed, field) in [
        ("not json".to_owned(), "target_directory"),
        (r#"{"target_directory": 1}"#.to_owned(), "target_directory"),
        (only_target.to_string(), "build_directory"),
    ] {
        assert_eq!(
            refused_with(&root, Some(printed)),
            Failure::new(
                format!("`{METADATA}` printed no `{field}`"),
                format!("run `{METADATA}` from the repo root to see what it prints")
            )
        );
    }
}

/// A `cargo metadata` that fails in the checkout stops the run before the warm-up, with what
/// it printed and the fix of any failed command there.
#[test]
fn a_failed_cargo_metadata_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-metadata-fails");
    let root = repo(&scratch);
    let mut fake = Fake {
        metadata_fails: Some("error: failed to parse manifest\n".to_owned()),
        ..Fake::default()
    };
    let refused = measure_with(&root, false, 1, &mut fake).expect_err("refused");
    assert_eq!(fake.ran("cargo build"), 0, "no warm-up");
    assert_eq!(fake.ran("git worktree add"), 0, "nothing made");
    assert_eq!(
        refused,
        Failure::new(
            "`cargo metadata` failed:\nerror: failed to parse manifest",
            format!("run `{METADATA}` from the repo root to see the error")
        )
    );
}

#[test]
fn a_target_debug_that_is_no_real_folder_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-debug-not-a-folder");
    let root = repo(&scratch);
    let debug = root.join("target").join("debug");
    let refusal = |what: &str| {
        Failure::new(
            format!(
                "refusing to measure: {} is {what}, so the worktree would not get its own copy \
                 of the build output",
                debug.display()
            ),
            format!(
                "move {} away (the warm-up, `cargo build -p clauseo-desktop`, then builds a real \
                 folder there), then run again",
                debug.display()
            ),
        )
    };
    Scratch::remove_dir_all(&debug).expect("no debug folder");
    Scratch::write(&debug, "a file").expect("a file instead");
    assert_eq!(refused_with(&root, None), refusal("not a folder"));
    // Windows makes a symlink only with a privilege the tests do not assume.
    #[cfg(unix)]
    {
        Scratch::remove_file(&debug).expect("no file");
        let elsewhere = scratch.join("elsewhere/debug");
        Scratch::create_dir_all(&elsewhere).expect("another checkout's output");
        Scratch::symlink_dir(&elsewhere, &debug).expect("a link to it");
        assert_eq!(
            refused_with(&root, None),
            refusal("a symlink, not a folder")
        );
    }
}

/// A `target/debug` that cannot be read for another reason than not being there stops the run:
/// with `target` a file, Unix says the path is not a folder (Windows may say it is not found,
/// which leaves it to the warm-up).
#[cfg(unix)]
#[test]
fn a_target_debug_that_cannot_be_read_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-debug-unreadable");
    let root = repo(&scratch);
    Scratch::remove_dir_all(root.join("target")).expect("no target folder");
    Scratch::write(root.join("target"), "a file").expect("a file instead");
    let debug = root.join("target").join("debug");
    let error = std::fs::symlink_metadata(&debug).expect_err("not readable");
    assert_eq!(
        refused_with(&root, None),
        Failure::new(
            format!("could not read {}: {error}", debug.display()),
            "fix what the error names, then run again"
        )
    );
}

/// A checkout never built has no `target/debug` yet: the warm-up makes a real one, so the checks
/// pass and the run goes on (here the fake warm-up makes none, so the copy then fails).
#[test]
fn a_target_debug_not_there_yet_is_left_to_the_warm_up() {
    let scratch = Scratch::new("worktree-no-debug-yet");
    let root = repo(&scratch);
    Scratch::remove_dir_all(root.join("target/debug")).expect("never built");
    let mut fake = Fake::default();
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    assert_eq!(
        fake.ran("cargo build -p clauseo-desktop -v"),
        1,
        "warmed up"
    );
    assert_eq!(
        find(&measured, "worktree_copy").outcome,
        Outcome::Failed(Failure::new(
            "target/debug is not in this checkout",
            "run `cargo build -p clauseo-desktop` from the repo root, then run again"
        ))
    );
}

#[test]
fn a_checkout_without_target_xtask_run_stops_the_run_before_the_warm_up() {
    let scratch = Scratch::new("worktree-no-xtask-run");
    let root = repo(&scratch);
    Scratch::remove_dir_all(root.join("target/xtask-run")).expect("no xtask-run");
    assert_eq!(
        refused_with(&root, None),
        Failure::new("target/xtask-run is not in this checkout", XTASK_RUN_FIX)
    );
    assert_eq!(
        XTASK_RUN_FIX,
        "run `cargo xtask --help` from the repo root (the alias builds xtask into \
         target/xtask-run), then run again"
    );
}

#[test]
fn the_own_target_is_target_in_the_root_spelled_either_way_and_made_or_not() {
    let scratch = Scratch::new("worktree-own-target");
    let root = scratch.join("repo");
    Scratch::create_dir_all(&root).expect("the root");
    assert!(own_target(&root.join("target"), &root), "not made yet");
    assert!(own_target(&root.join("target/"), &root));
    assert!(!own_target(&root.join("out"), &root));
    assert!(!own_target(&scratch.join("target"), &root));
    assert!(!own_target(&root.join("target/debug"), &root));
    #[cfg(unix)]
    {
        Scratch::symlink_dir(&root, scratch.join("link")).expect("a link to the root");
        assert!(
            own_target(&scratch.join("link/target"), &root),
            "the same folder through a link"
        );
    }
}
