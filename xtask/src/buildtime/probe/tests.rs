//! Tests for `probe.rs`: the restore of the marker file on every path, the guard against edits made
//! during a run, and the warm-up judgement.

use super::*;
use crate::scratch::Scratch;
use std::panic::{AssertUnwindSafe, catch_unwind};

const REL: &str = "crates/desktop/src/lib.rs";
/// The guard's temporary file for the marker file: next to it, and under the backup folder.
const TMP: &str = "crates/desktop/src/.lib.rs.buildtime.tmp";
const ORIGINAL: &str = "//! desktop\n\npub const BUILDTIME_MARKER: u64 = 41;\n\npub fn f() {}\n";
const BUMPED: &str = "//! desktop\n\npub const BUILDTIME_MARKER: u64 = 42;\n\npub fn f() {}\n";
const RUSTC: &str = "     Running `/rust/bin/rustc --crate-name clauseo_desktop --edition=2024 crates/desktop/src/lib.rs --crate-type lib -C embed-bitcode=no -C incremental=/ws/target/debug/incremental -C metadata=4a2f`";
const RUSTC_NOT_INCREMENTAL: &str = "     Running `/rust/bin/rustc --crate-name clauseo_desktop --edition=2024 crates/desktop/src/lib.rs --crate-type lib -C embed-bitcode=no -C metadata=4a2f`";
const CHECK: Probe = Probe {
    name: "check_desktop",
    package: "clauseo-desktop",
    budget_id: "cargo_check_app_edit",
    subcommand: "check",
};

/// A scratch repo for one test, holding the marker file. It is removed when the test ends, also
/// when the test fails. `name` must be unique among these tests (see `Scratch::new`).
fn scratch(name: &str) -> Scratch {
    let dir = Scratch::new(&format!("buildtime-{name}"));
    let file = dir.join(REL);
    Scratch::create_dir_all(file.parent().expect("the file has a parent"))
        .expect("create the crate dir");
    Scratch::write(&file, ORIGINAL).expect("write the marker file");
    dir
}

/// Saves the marker file's original through the guard, as \`measure\` does.
fn save(dir: &Path) -> Result<Guard, crate::write::GuardFailure> {
    Guard::save(dir, BACKUPS, REL, ORIGINAL)
}

fn marker() -> Marker {
    Marker {
        path: REL.into(),
        text: ORIGINAL.into(),
    }
}

fn file_text(dir: &Path) -> String {
    fs::read_to_string(dir.join(REL)).expect("the marker file is readable")
}

fn backup_of(dir: &Path) -> PathBuf {
    dir.join(BACKUP_DIR).join(REL)
}

#[test]
fn a_failing_cargo_puts_the_original_bytes_back_and_leaves_no_copy() {
    let dir = scratch("failing-cargo");
    let mut runner = |_: &Cmd| -> io::Result<Ran> {
        Ok(Ran::exited(false, "error[E0432]: unresolved import\n", ""))
    };
    let failure = measure(&dir, &marker(), CHECK, 2, false, &mut runner)
        .expect_err("a failing cargo fails the probe");
    assert_eq!(failure.message, "cargo check -v failed");
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_spawn_error_puts_the_original_bytes_back_and_leaves_no_copy() {
    let dir = scratch("spawn-error");
    let mut runner = |_: &Cmd| -> io::Result<Ran> {
        Err(io::Error::new(io::ErrorKind::NotFound, "no cargo here"))
    };
    let failure = measure(&dir, &marker(), CHECK, 2, false, &mut runner)
        .expect_err("a spawn error fails the probe");
    assert!(failure.message.contains("no cargo here"));
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_cargo_past_its_limit_says_it_was_stopped_and_puts_the_original_bytes_back() {
    let dir = scratch("timed-out-cargo");
    let mut runner = |_: &Cmd| -> io::Result<Ran> { Ok(Ran::timed_out()) };
    let failure = measure(&dir, &marker(), CHECK, 2, false, &mut runner)
        .expect_err("a cargo past its limit fails the probe");
    assert_eq!(
        failure.message,
        "`cargo check -v` did not finish within 1 h, so it was stopped"
    );
    assert_eq!(
        failure.fix,
        "run `cargo check -v` by hand to see why it does not finish within 1 h"
    );
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_panicking_runner_restores_the_file_on_the_spot() {
    let dir = scratch("panicking-runner");
    let mut runner = |_: &Cmd| -> io::Result<Ran> { panic!("cargo exploded") };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        measure(&dir, &marker(), CHECK, 2, false, &mut runner)
    }));
    assert!(outcome.is_err(), "the panic reaches the caller");
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_leftover_copy_is_put_back_and_deleted_and_named() {
    let dir = scratch("leftover");
    Scratch::write(dir.join(REL), BUMPED).expect("the interrupted run left the bump");
    Scratch::create_dir_all(backup_of(&dir).parent().expect("has a parent"))
        .expect("create the backup dir");
    Scratch::write(backup_of(&dir), ORIGINAL).expect("the interrupted run left the copy");
    let lines = restore_leftovers(&dir).expect("the leftover restores");
    assert_eq!(
        lines,
        vec![format!("restored {REL} from an interrupted buildtime run")]
    );
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
    assert!(!dir.join(BACKUP_DIR).join("crates").exists());
}

#[test]
fn no_leftover_copies_means_nothing_is_printed() {
    let dir = scratch("no-leftovers");
    assert_eq!(restore_leftovers(&dir), Ok(Vec::new()));
    assert_eq!(file_text(&dir), ORIGINAL);
}

#[test]
fn a_successful_run_returns_the_samples_and_leaves_the_original_bytes() {
    let dir = scratch("success");
    let mut calls = Vec::new();
    let mut runner = |cmd: &Cmd| -> io::Result<Ran> {
        calls.push(cmd.to_string());
        let output = if cmd.args.iter().any(|arg| arg == "-v") {
            RUSTC.to_owned()
        } else {
            String::new()
        };
        Ok(Ran::exited(true, output, ""))
    };
    let samples =
        measure(&dir, &marker(), CHECK, 3, false, &mut runner).expect("the probe measures");
    assert_eq!(samples.secs.len(), 3);
    assert_eq!(samples.warning, None);
    assert_eq!(
        calls,
        vec![
            "cargo check -v",
            "cargo check",
            "cargo check",
            "cargo check"
        ]
    );
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_crate_that_is_not_rebuilt_fails_before_any_timed_run_and_restores() {
    let dir = scratch("not-rebuilt");
    let mut calls = 0;
    let mut runner = |_: &Cmd| -> io::Result<Ran> {
        calls += 1;
        Ok(Ran::exited(true, "    Fresh clauseo-desktop v0.1.0\n", ""))
    };
    let failure = measure(&dir, &marker(), CHECK, 5, false, &mut runner)
        .expect_err("a crate cargo did not rebuild fails the probe");
    assert_eq!(
        failure.message,
        "`cargo check` did not rebuild clauseo-desktop after its marker changed; it is not in the default members' build graph"
    );
    assert_eq!(
        failure.fix,
        "add clauseo-desktop to the build graph of the default members (default-members in Cargo.toml)"
    );
    assert_eq!(calls, 1, "only the warm-up runs");
    assert_eq!(file_text(&dir), ORIGINAL);
}

#[test]
fn a_non_incremental_build_warns_locally_and_is_refused_in_ci() {
    let dir = scratch("not-incremental");
    let mut runner = |cmd: &Cmd| -> io::Result<Ran> {
        let output = if cmd.args.iter().any(|arg| arg == "-v") {
            RUSTC_NOT_INCREMENTAL.to_owned()
        } else {
            String::new()
        };
        Ok(Ran::exited(true, output, ""))
    };
    let samples =
        measure(&dir, &marker(), CHECK, 1, false, &mut runner).expect("locally it is measured");
    assert_eq!(
        samples.warning.as_deref(),
        Some(
            "warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget"
        )
    );
    assert_eq!(file_text(&dir), ORIGINAL);

    let refused =
        measure(&dir, &marker(), CHECK, 1, true, &mut runner).expect_err("in CI it is refused");
    assert_eq!(
        refused.message,
        "refusing to measure: CI is set and clauseo-desktop builds without incremental"
    );
    assert_eq!(refused.fix, INCREMENTAL_FIX);
    assert_eq!(file_text(&dir), ORIGINAL);
}

#[test]
fn the_warm_up_judgement_covers_present_absent_and_not_incremental() {
    assert_eq!(judge_warm_up(RUSTC, CHECK, true), Ok(None));
    assert!(judge_warm_up("", CHECK, false).is_err());
    let warning = judge_warm_up(RUSTC_NOT_INCREMENTAL, CHECK, false);
    assert_eq!(
            warning,
            Ok(Some(
                "warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget".to_owned()
            ))
        );
    let refused = judge_warm_up(RUSTC_NOT_INCREMENTAL, CHECK, true).expect_err("CI refuses");
    assert_eq!(refused.fix, INCREMENTAL_FIX);
}

/// The repo file as an interrupted run left it: the marker bumped, and one real edit elsewhere.
const EDITED_AND_BUMPED: &str = "//! desktop, edited by hand\n\npub const BUILDTIME_MARKER: u64 = 42;\n\npub fn f() -> u8 { 1 }\n";

#[test]
fn a_conflict_leaves_both_files_untouched_and_names_the_copy() {
    let dir = scratch("conflict");
    Scratch::write(dir.join(REL), EDITED_AND_BUMPED).expect("the user edited the file");
    Scratch::create_dir_all(backup_of(&dir).parent().expect("has a parent"))
        .expect("create the backup dir");
    Scratch::write(backup_of(&dir), ORIGINAL).expect("the interrupted run left the copy");
    let failure = restore_leftovers(&dir).expect_err("a changed file is a conflict");
    assert_eq!(
        failure.message,
        format!(
            "{REL} changed after an interrupted buildtime run; its saved copy is target/xtask/buildtime/{REL}"
        )
    );
    assert_eq!(
        failure.fix,
        format!("compare the two, keep what you want, then delete target/xtask/buildtime/{REL}")
    );
    assert_eq!(file_text(&dir), EDITED_AND_BUMPED);
    assert_eq!(
        fs::read_to_string(backup_of(&dir)).expect("the copy is kept"),
        ORIGINAL
    );
}

#[test]
fn a_missing_file_is_a_conflict_and_the_copy_is_kept() {
    let dir = scratch("missing");
    Scratch::remove_file(dir.join(REL)).expect("the file is gone");
    Scratch::create_dir_all(backup_of(&dir).parent().expect("has a parent"))
        .expect("create the backup dir");
    Scratch::write(backup_of(&dir), ORIGINAL).expect("the interrupted run left the copy");
    let failure = restore_leftovers(&dir).expect_err("a missing file is a conflict");
    assert!(failure.message.starts_with(&format!("{REL} is missing")));
    assert!(backup_of(&dir).exists());
    assert!(!dir.join(REL).exists());
}

#[test]
fn a_stray_file_without_a_marker_is_ignored() {
    let dir = scratch("stray");
    let stray = dir.join(BACKUP_DIR).join(".DS_Store");
    Scratch::create_dir_all(dir.join(BACKUP_DIR)).expect("create the backup dir");
    Scratch::write(&stray, "Bud1 junk").expect("the stray file is there");
    assert_eq!(restore_leftovers(&dir), Ok(Vec::new()));
    assert_eq!(
        fs::read_to_string(&stray).expect("the stray file is left alone"),
        "Bud1 junk"
    );
    assert_eq!(file_text(&dir), ORIGINAL);
}

#[test]
fn a_second_save_of_the_same_file_fails_and_leaves_the_file_alone() {
    let dir = scratch("second-save");
    let mut first = save(&dir).expect("the first save");
    assert!(
        !dir.join(BACKUP_DIR).join(TMP).exists(),
        "the temporary file is gone after a save"
    );
    first
        .write(BUMPED.to_owned())
        .expect("the probe edits the file");
    let Err(failure) = save(&dir) else {
        panic!("a second save is refused");
    };
    assert_eq!(
        failure.message,
        format!(
            "a saved copy of {REL} already exists at target/xtask/buildtime/{REL}; an earlier buildtime run kept it (a conflict or a failed restore)"
        )
    );
    assert_eq!(
        failure.fix,
        format!(
            "compare {REL} with target/xtask/buildtime/{REL}, keep what you want, then delete target/xtask/buildtime/{REL}"
        ),
        "the same fix as a conflict: it never says to copy the original over the file"
    );
    assert_eq!(file_text(&dir), BUMPED, "the file is not touched");
    assert_eq!(
        fs::read_to_string(backup_of(&dir)).expect("the first copy is kept"),
        ORIGINAL
    );
    first.restore().expect("the first guard restores");
    assert_eq!(file_text(&dir), ORIGINAL);
    assert!(!backup_of(&dir).exists());
}

#[test]
fn the_temporary_file_of_an_interrupted_save_is_deleted() {
    let dir = scratch("tmp-leftover");
    let tmp = dir.join(BACKUP_DIR).join(TMP);
    Scratch::create_dir_all(tmp.parent().expect("has a parent")).expect("create the backup dir");
    Scratch::write(&tmp, ORIGINAL).expect("the interrupted save left its temporary file");
    assert_eq!(restore_leftovers(&dir), Ok(Vec::new()));
    assert!(!tmp.exists());
    assert_eq!(file_text(&dir), ORIGINAL);
}

#[test]
fn the_temporary_file_of_an_interrupted_write_goes_with_the_restore() {
    let dir = scratch("write-tmp-leftover");
    Scratch::create_dir_all(backup_of(&dir).parent().expect("a parent")).expect("the backup dir");
    Scratch::write(backup_of(&dir), ORIGINAL).expect("the copy a killed run left");
    Scratch::write(dir.join(REL), BUMPED).expect("its bump");
    let tmp = dir.join(TMP);
    Scratch::write(&tmp, "half a bump").expect("the killed write's temporary file");
    assert_eq!(
        restore_leftovers(&dir),
        Ok(vec![format!(
            "restored {REL} from an interrupted buildtime run"
        )])
    );
    assert!(!tmp.exists());
    assert_eq!(file_text(&dir), ORIGINAL);
}

/// The file as someone saved it while the probe ran.
const EDITED_MID_RUN: &str = "//! desktop\n\npub const BUILDTIME_MARKER: u64 = 42;\n\npub fn f() {}\n// a line typed mid-run\n";

#[test]
fn an_edit_during_the_run_survives_and_keeps_the_copy_and_fails_the_probe() {
    let dir = scratch("edit-mid-run");
    let file = dir.join(REL);
    let mut calls = 0;
    let mut runner = |cmd: &Cmd| -> io::Result<Ran> {
        calls += 1;
        if calls == 2 {
            Scratch::write(&file, EDITED_MID_RUN).expect("someone saves the file");
        }
        let output = if cmd.args.iter().any(|arg| arg == "-v") {
            RUSTC.to_owned()
        } else {
            String::new()
        };
        Ok(Ran::exited(true, output, ""))
    };
    let failure = measure(&dir, &marker(), CHECK, 3, false, &mut runner)
        .expect_err("an edit during the run fails the probe");
    assert_eq!(calls, 2, "no run starts after the edit is found");
    assert_eq!(
        failure.message,
        format!(
            "{REL} changed during this buildtime run; its saved copy is target/xtask/buildtime/{REL}"
        )
    );
    assert_eq!(
        failure.fix,
        format!("compare the two, keep what you want, then delete target/xtask/buildtime/{REL}")
    );
    assert_eq!(
        fs::read(dir.join(REL)).expect("the file is readable"),
        EDITED_MID_RUN.as_bytes()
    );
    assert_eq!(
        fs::read_to_string(backup_of(&dir)).expect("the copy is kept"),
        ORIGINAL
    );
}

#[test]
fn an_edit_before_the_restore_survives_and_keeps_the_copy() {
    let dir = scratch("edit-before-restore");
    let mut guard = save(&dir).expect("the save");
    guard.write(BUMPED.to_owned()).expect("the bump");
    Scratch::write(dir.join(REL), EDITED_MID_RUN).expect("someone saves the file");
    let failure = guard.restore().expect_err("the restore finds the edit");
    assert!(
        failure
            .message
            .starts_with(&format!("{REL} changed during this"))
    );
    assert_eq!(file_text(&dir), EDITED_MID_RUN);
    assert_eq!(fs::read_to_string(backup_of(&dir)).expect("kept"), ORIGINAL);
}

#[test]
fn a_dropped_guard_restores_only_the_bytes_it_wrote() {
    let dir = scratch("drop-after-edit");
    let mut guard = save(&dir).expect("the save");
    guard.write(BUMPED.to_owned()).expect("the bump");
    Scratch::write(dir.join(REL), EDITED_MID_RUN).expect("someone saves the file");
    drop(guard);
    assert_eq!(
        file_text(&dir),
        EDITED_MID_RUN,
        "the edit is not overwritten"
    );
    assert_eq!(fs::read_to_string(backup_of(&dir)).expect("kept"), ORIGINAL);

    let dir = scratch("drop-untouched");
    let mut guard = save(&dir).expect("the save");
    guard.write(BUMPED.to_owned()).expect("the bump");
    drop(guard);
    assert_eq!(file_text(&dir), ORIGINAL, "the guard's own bump is undone");
    assert!(!backup_of(&dir).exists());
}

#[test]
fn a_kept_copy_marks_its_crate_so_later_probes_on_it_are_skipped() {
    let dir = scratch("kept-copy");
    let crate_dir = REL.split("/src/").next().expect("REL is inside a crate");
    assert!(!holds_kept_copy(&dir, crate_dir), "no copy yet");
    let _kept = save(&dir).expect("the save");
    assert!(holds_kept_copy(&dir, crate_dir), "the copy is there");
    assert!(
        !holds_kept_copy(&dir, "crates/other"),
        "another crate holds none"
    );
}

#[test]
fn the_scripts_app_copy_and_its_temporary_file_are_left_to_the_script() {
    let dir = scratch("buildtime-app-copy");
    let app_copy = dir.join(BACKUP_DIR).join("ui/app/src/App.tsx");
    let app_tmp = dir
        .join(BACKUP_DIR)
        .join("ui/app/src/.App.tsx.buildtime.tmp");
    let marker_tmp = dir.join(BACKUP_DIR).join("ui/app/src/lib.rs.tmp");
    Scratch::create_dir_all(app_copy.parent().expect("a parent")).expect("create the copy dir");
    // Even a copy that looks like a marker file is not read as one.
    Scratch::write(&app_copy, ORIGINAL).expect("write the App.tsx copy");
    Scratch::write(&app_tmp, "half").expect("write the temporary file");
    Scratch::write(&marker_tmp, "half").expect("write a .tmp file");
    assert_eq!(restore_leftovers(&dir), Ok(Vec::new()));
    assert_eq!(
        fs::read_to_string(&app_copy).ok().as_deref(),
        Some(ORIGINAL)
    );
    assert!(app_tmp.exists() && marker_tmp.exists());
    assert!(
        !dir.join("ui/app/src/App.tsx").exists(),
        "nothing is restored"
    );
}
