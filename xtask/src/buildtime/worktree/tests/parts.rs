//! Tests of the parts of a worktree run: the copy, the incremental judgement, the JSON object, the
//! worktree's place, and how cargo's JSON names what it rebuilt, read from a real cargo build.

use super::super::steps::XTASK_RUN_FIX;
use super::super::steps::xtask_binary;
use super::super::*;
use super::{Fake, LIMIT, app_only, artifact, find, left_behind, measure_with, repo};
use crate::buildtime::INCREMENTAL_FIX;
use crate::process::{self, Cmd, Program, TEST_TIMEOUT};
use crate::scratch::Scratch;
use std::fs::FileTimes;
use std::time::{Duration, SystemTime};

#[test]
fn a_checkout_without_its_build_output_fails_the_copy_and_copies_nothing() {
    let scratch = Scratch::new("worktree-no-output");
    let root = scratch.join("repo");
    let path = scratch.join("new");
    let worktree = Worktree {
        root: &root,
        path: path.clone(),
        ci: false,
    };
    let debug_fix = "run `cargo build -p clauseo-desktop` from the repo root, then run again";
    assert_eq!(
        worktree.copy(),
        Err(Failure::new(
            "target/debug is not in this checkout",
            debug_fix
        ))
    );
    Scratch::create_dir_all(root.join("target/debug")).expect("debug");
    assert_eq!(
        worktree.copy(),
        Err(Failure::new(
            "target/xtask-run is not in this checkout",
            XTASK_RUN_FIX
        ))
    );
    assert!(!path.exists(), "nothing was copied");
    Scratch::create_dir_all(root.join("target/xtask-run")).expect("xtask-run");
    assert_eq!(
        worktree.copy(),
        Ok(()),
        "a checkout without CACHEDIR.TAG copies the rest"
    );
    assert!(path.join("target/xtask-run").is_dir() && !path.join("target/CACHEDIR.TAG").exists());
}

/// The rustc lines of a build of the app, with or without incremental.
fn rustc_line(incremental: bool) -> String {
    let flag = if incremental {
        " -C incremental=/w/target/debug/incremental"
    } else {
        ""
    };
    format!("     Running `rustc --crate-name clauseo_desktop --edition=2024 src/lib.rs{flag}`\n")
}

#[test]
fn a_build_without_incremental_is_refused_in_ci_and_warned_about_elsewhere() {
    let scratch = Scratch::new("worktree-incremental");
    let root = repo(&scratch);
    let mut warm = Fake {
        warm_up_stderr: rustc_line(false),
        ..Fake::default()
    };
    let refused = measure_with(&root, true, 1, &mut warm).expect_err("refused in CI");
    assert_eq!(
        refused,
        Failure::new(
            "refusing to measure: CI is set and clauseo-desktop builds without incremental",
            INCREMENTAL_FIX
        )
    );
    assert_eq!(
        warm.ran("git worktree add"),
        0,
        "refused before anything is made"
    );
    let mut warm_ok = Fake {
        warm_up_stderr: rustc_line(true),
        builds: vec![app_only()],
        build_stderr: rustc_line(false),
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut warm_ok).expect("measured");
    let Outcome::Measured { warning, .. } = &find(&measured, TOTAL).outcome else {
        panic!("measured");
    };
    assert_eq!(
        warning.as_deref(),
        Some(
            "warning: clauseo-desktop builds without incremental; its timing is not comparable \
             with the budget"
        )
    );
    let mut in_ci = Fake {
        builds: vec![app_only()],
        build_stderr: rustc_line(false),
        ..Fake::default()
    };
    let measured = measure_with(&root, true, 3, &mut in_ci).expect("measured");
    assert_eq!(
        find(&measured, "worktree_first_build").outcome,
        Outcome::Failed(Failure::new(
            "refusing to measure: CI is set and clauseo-desktop builds without incremental",
            INCREMENTAL_FIX
        ))
    );
    assert_eq!(
        in_ci.ran("git worktree add"),
        1,
        "every run would fail the same way, so none follows"
    );
    assert!(
        !left_behind(&root, &place(&root).expect("a place")),
        "removed"
    );
    assert!(!measured.passed(true));
}

#[test]
fn a_binary_missing_after_the_copy_fails_the_last_step() {
    let scratch = Scratch::new("worktree-no-binary");
    let root = repo(&scratch);
    Scratch::remove_file(xtask_binary(&root)).expect("no binary");
    let mut fake = Fake {
        builds: vec![app_only()],
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    let copied = xtask_binary(&place(&root).expect("a place"));
    assert_eq!(
        find(&measured, "worktree_first_xtask").outcome,
        Outcome::Failed(Failure::new(
            format!("{} is not there after the copy", copied.display()),
            XTASK_RUN_FIX
        ))
    );
    assert_eq!(fake.ran("cargo xtask"), 0);
}

#[test]
fn a_failed_command_shows_its_output_and_a_fix_that_fits_where_it_ran() {
    let scratch = Scratch::new("worktree-failed-commands");
    let root = repo(&scratch);
    let mut warm = Fake {
        warm_up_fails: Some(
            "       Fresh libc v0.2.177\n     Running `rustc --crate-name x`\nerror: could not \
             compile `clauseo-desktop`\n"
                .to_owned(),
        ),
        ..Fake::default()
    };
    let refused = measure_with(&root, false, 1, &mut warm).expect_err("the warm-up failed");
    assert_eq!(
        refused,
        Failure {
            message: "`cargo build -p clauseo-desktop -v` failed".into(),
            fix: "run `cargo build -p clauseo-desktop -v` from the repo root to see the error"
                .into(),
            output: Some("error: could not compile `clauseo-desktop`\n".into()),
        }
    );
    let mut install = Fake {
        builds: vec![app_only()],
        pnpm_fails: Some("ERR_PNPM_OUTDATED_LOCKFILE\n".to_owned()),
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut install).expect("measured");
    let Outcome::Failed(failure) = &find(&measured, "worktree_pnpm_install").outcome else {
        panic!("the install failed");
    };
    assert_eq!(
        failure.fix,
        "read its output above (the worktree is removed once the run ends), fix what it names, \
         then run again"
    );
    assert_eq!(
        failure.output.as_deref(),
        Some("ERR_PNPM_OUTDATED_LOCKFILE\n")
    );
    assert_eq!(
        find(&measured, "worktree_first_build").outcome,
        Outcome::Skipped(NOT_REACHED.into())
    );
}

#[test]
fn the_json_has_the_run_fields_and_errors_only_when_there_are_some() {
    let incremental = Incremental {
        on: true,
        source: "cargo default".into(),
    };
    let mut measured = Measured {
        results: vec![ProbeResult {
            name: TOTAL,
            unit: Unit::S,
            reported_only: false,
            outcome: Outcome::of_samples(TOTAL, vec![31.0], Some(LIMIT)),
        }],
        errors: Vec::new(),
        notes: vec!["run 1, first build: rebuilt nothing; fresh: 1".to_owned()],
    };
    let keys = |json: &str| {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid json");
        let mut keys: Vec<String> = value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort_unstable();
        (keys, value)
    };
    let (names, value) = keys(&json_object(
        "rustc 1.99.0",
        &incremental,
        true,
        1,
        true,
        &measured,
    ));
    assert_eq!(
        names,
        [
            "ci",
            "incremental",
            "passed",
            "probes",
            "profile",
            "runs",
            "rustc"
        ]
    );
    assert_eq!(value["probes"][0]["status"], "over");
    assert_eq!(value["probes"][0]["budget_id"], "fresh_agent_worktree");
    assert!(measured.passed(false), "an alert gate only reports");
    measured
        .errors
        .push(Failure::new("could not remove it", "remove it"));
    let (names, value) = keys(&json_object(
        "rustc 1.99.0",
        &incremental,
        true,
        1,
        false,
        &measured,
    ));
    assert!(names.contains(&"errors".to_owned()));
    assert_eq!(value["errors"], serde_json::json!(["could not remove it"]));
    assert!(!measured.passed(false));
}

#[test]
fn the_worktree_goes_next_to_the_root_and_a_root_with_no_parent_is_refused() {
    let scratch = Scratch::new("worktree-place");
    let root = scratch.join("clauseo-acp");
    let parent = dunce::canonicalize(&*scratch).expect("the scratch dir");
    assert_eq!(
        place(&root),
        Ok(parent.join("clauseo-acp-buildtime-worktree"))
    );
    let refused = place(Path::new("/")).expect_err("no parent");
    assert_eq!(
        refused,
        Failure::new(
            "cannot place the worktree next to /: it has no parent folder",
            "run `cargo xtask buildtime worktree` from a checkout inside a folder: the worktree \
             goes next to the checkout"
        )
    );
    let gone = scratch.join("gone");
    let error = dunce::canonicalize(&gone).expect_err("not there");
    let refused = place(&gone.join("clauseo-acp")).expect_err("no such folder");
    assert_eq!(
        refused,
        Failure::new(
            format!(
                "cannot place the worktree next to {0}: {1}: {error}",
                gone.join("clauseo-acp").display(),
                gone.display()
            ),
            "fix what the error names in that folder, where the worktree goes, then run again"
        )
    );
}

#[test]
fn two_paths_are_one_folder_when_they_are_equal_or_resolve_to_one() {
    let scratch = Scratch::new("worktree-same-path");
    let gone = scratch.join("gone");
    assert!(same_path(&gone, &gone), "equal, though not on disk");
    let real = scratch.join("real");
    Scratch::create_dir_all(&real).expect("a folder");
    assert!(!same_path(&real, &gone));
    #[cfg(unix)]
    {
        Scratch::symlink_dir(&real, scratch.join("link")).expect("a link to it");
        assert!(
            same_path(&scratch.join("link"), &real),
            "both resolve to one folder"
        );
    }
}

#[test]
fn a_failure_names_what_its_command_printed_when_it_printed_anything() {
    let failed = |output: Option<&str>| Failure {
        message: "`git worktree remove --force /w` failed".into(),
        fix: "f".into(),
        output: output.map(str::to_owned),
    };
    assert_eq!(
        with_output(failed(Some("fatal: '/w' is not\n a working tree\n"))),
        "`git worktree remove --force /w` failed (fatal: '/w' is not a working tree)"
    );
    for quiet in [None, Some(""), Some(" \n")] {
        assert_eq!(
            with_output(failed(quiet)),
            "`git worktree remove --force /w` failed",
            "{quiet:?}"
        );
    }
}

#[test]
fn a_package_id_names_its_package_in_every_form_cargo_prints() {
    for (id, name) in [
        (
            "path+file:///w/crates/desktop#clauseo-desktop@0.0.0",
            "clauseo-desktop",
        ),
        ("path+file:///w/dep#0.1.0", "dep"),
        ("path+file:///C:/w/dep/#0.1.0", "dep"),
        (
            "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.228",
            "serde",
        ),
        ("git+https://github.com/o/tao?branch=dev#0.34.0", "tao"),
        (
            "serde 1.0.228 (registry+https://github.com/rust-lang/crates.io-index)",
            "serde",
        ),
    ] {
        assert_eq!(package_name(id), name, "{id}");
    }
}

/// cargo's JSON from a real build of a scratch workspace: `app-desk` in `crates/desk`, whose id
/// carries its name, and `dep` in `dep`, whose id does not.
#[test]
fn a_real_cargo_build_names_each_package_it_rebuilt_besides_the_one_asked() {
    let dir = Scratch::new("worktree-cargo-json");
    let package = |name: &str, deps: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{deps}")
    };
    let files = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/desk\", \"dep\"]\nresolver = \"3\"\n".to_owned(),
        ),
        (
            "crates/desk/Cargo.toml",
            package(
                "app-desk",
                "[dependencies]\ndep = { path = \"../../dep\" }\n",
            ),
        ),
        (
            "crates/desk/src/lib.rs",
            "pub fn f() -> u8 { dep::g() }\n".to_owned(),
        ),
        ("dep/Cargo.toml", package("dep", "")),
        ("dep/src/lib.rs", "pub fn g() -> u8 { 1 }\n".to_owned()),
    ];
    for (file, text) in files {
        let path = dir.join(file);
        Scratch::create_dir_all(path.parent().expect("a folder")).expect("folders");
        Scratch::write(path, text).expect("a file");
    }
    let build = || {
        let args = [
            "build",
            "-p",
            "app-desk",
            "--message-format=json",
            "--offline",
        ];
        let built = process::run_quiet(&Cmd::new(Program::Cargo, args, TEST_TIMEOUT), &dir)
            .expect("cargo runs");
        assert!(built.success(), "{}", built.stderr);
        built.stdout
    };
    let cold = build();
    assert_eq!(rebuilt_besides(&cold, "app-desk"), ["dep (lib)"], "cold");
    assert_eq!(
        built_line(&cold),
        "first build: rebuilt dep (lib), app-desk (lib); fresh: 0"
    );
    let warm = build();
    assert!(
        rebuilt_besides(&warm, "app-desk").is_empty() && rebuilt_besides(&warm, "dep").is_empty()
    );
    assert_eq!(built_line(&warm), "first build: rebuilt nothing; fresh: 2");
    // 2100-01-01, 47482 days after the epoch: later than any build.
    let later =
        FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_hours(47_482 * 24));
    Scratch::set_times(dir.join("crates/desk/src/lib.rs"), later).expect("an edit");
    let edited = build();
    assert!(
        rebuilt_besides(&edited, "app-desk").is_empty(),
        "only the app rebuilt"
    );
    assert_eq!(rebuilt_besides(&edited, "dep"), ["app-desk (lib)"]);
    assert_eq!(
        built_line(&edited),
        "first build: rebuilt app-desk (lib); fresh: 1"
    );
}

/// A unit cargo names twice, as a build that reports an artifact again does, is named once; every
/// fresh message counts.
#[test]
fn the_built_line_names_each_rebuilt_unit_once_and_counts_the_fresh_ones() {
    let twice = format!("{}\n{}", app_only(), app_only());
    assert_eq!(
        built_line(&twice),
        "first build: rebuilt clauseo-desktop (custom-build), clauseo-desktop (lib); fresh: 2"
    );
    assert_eq!(built_line(""), "first build: rebuilt nothing; fresh: 0");
}

/// cargo prints a `Dirty` line only for a unit it built before (cargo 1.99: a unit whose
/// build output is missing gets only its `Compiling` line), so a first build may rebuild another
/// package and print none.
#[test]
fn a_first_build_that_rebuilt_another_package_without_a_dirty_line_says_it_built_it_first() {
    let scratch = Scratch::new("worktree-rebuilt-no-dirty");
    let root = repo(&scratch);
    let more = format!(
        "{}\n{}",
        artifact("path+file:///w/dep#0.1.0", "lib", false),
        app_only()
    );
    let mut fake = Fake {
        builds: vec![more],
        build_stderr: "   Compiling dep v0.1.0 (/w/dep)\n       Fresh libc v0.2.177\n".to_owned(),
        ..Fake::default()
    };
    let measured = measure_with(&root, false, 1, &mut fake).expect("measured");
    assert_eq!(
        find(&measured, "worktree_first_build").outcome,
        Outcome::Failed(Failure::new(
            "`cargo build -p clauseo-desktop --message-format=json -v` rebuilt more than \
             clauseo-desktop in the worktree: dep (lib)",
            NO_DIRTY_FIX
        ))
    );
    assert_eq!(
        NO_DIRTY_FIX,
        "cargo printed no `Dirty` line, so it built each one for the first time: the copied \
         target/debug held no build of them with the worktree's settings (features, environment, \
         `.cargo/config.toml`); find what differs from the repo root, then run again"
    );
    assert!(measured.notes.is_empty(), "no build passed");
}
