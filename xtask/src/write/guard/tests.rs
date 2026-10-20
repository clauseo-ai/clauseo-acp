//! Tests for the guard's own rules: its refusals, its temporary files, and the exact text of each
//! failure. `buildtime/probe/tests.rs` tests it through the probe.

use super::*;
use crate::scratch::Scratch;

const TOOL: Backups = Backups {
    dir: "target/xtask/guarded",
    tool: "guardtest",
};
const REL: &str = "src/lib.rs";
const COPY: &str = "target/xtask/guarded/src/lib.rs";
const ORIGINAL: &str = "original\n";
const EDIT: &str = "edit\n";

/// A scratch repo holding `REL` with `ORIGINAL`.
fn repo(name: &str) -> Scratch {
    let dir = Scratch::new(&format!("write-guard-{name}"));
    Scratch::create_dir_all(dir.join("src")).expect("create src");
    Scratch::write(dir.join(REL), ORIGINAL).expect("write the file");
    dir
}

fn text(path: &Path) -> String {
    fs::read_to_string(path).expect("read a file")
}

#[test]
fn a_write_goes_through_a_temporary_file_and_leaves_none() {
    let dir = repo("tmp");
    let tmp = dir.join("src/.lib.rs.guardtest.tmp");
    // A kill during an earlier write left its temporary file; the next write replaces it.
    Scratch::write(&tmp, "half").expect("a stale temporary file");
    let mut guard = Guard::save(&dir, TOOL, REL, ORIGINAL).expect("the save");
    guard.write(EDIT.to_owned()).expect("the write");
    assert_eq!(text(&dir.join(REL)), EDIT);
    assert!(
        !tmp.exists(),
        "the temporary file was renamed over the file"
    );
    guard.restore().expect("the restore");
    assert_eq!(text(&dir.join(REL)), ORIGINAL);
    assert!(!tmp.exists() && !dir.join(COPY).exists());
}

#[test]
fn a_path_outside_the_repo_is_refused_before_anything_is_written() {
    let dir = repo("refuse");
    for rel in ["../outside.rs", "/etc/hosts", "", "src/../../x.rs"] {
        let Err(failure) = Guard::save(&dir, TOOL, rel, ORIGINAL) else {
            panic!("{rel} is refused");
        };
        assert_eq!(
            failure,
            GuardFailure::new(
                format!("refusing to guard `{rel}`: it is not a relative path inside the repo"),
                "pass the file's repo path, such as crates/desktop/src/lib.rs".to_owned(),
            )
        );
        assert_eq!(
            TOOL.restore_leftover(&dir, rel),
            Err(failure.clone()),
            "{rel}"
        );
        assert_eq!(
            TOOL.discard_interrupted_write(&dir, rel),
            Err(failure),
            "{rel}"
        );
    }
    assert!(!dir.join("target").exists());
}

#[test]
fn a_temporary_file_left_by_an_interrupted_save_stops_the_next_save() {
    let dir = repo("stale-save");
    let tmp = dir.join("target/xtask/guarded/src/.lib.rs.guardtest.tmp");
    Scratch::create_dir_all(tmp.parent().expect("a parent")).expect("create the backup dir");
    Scratch::write(&tmp, "orig").expect("half a copy");
    let Err(failure) = Guard::save(&dir, TOOL, REL, ORIGINAL) else {
        panic!("the save stops");
    };
    let tmp_rel = "target/xtask/guarded/src/.lib.rs.guardtest.tmp";
    assert_eq!(
        failure,
        GuardFailure::new(
            format!("could not back up {REL}: {tmp_rel} is left from an interrupted save"),
            format!("compare {tmp_rel} with {REL}, keep what you want, then delete {tmp_rel}"),
        )
    );
    assert_eq!(text(&tmp), "orig", "the temporary file is kept");
    assert!(!dir.join(COPY).exists());

    // Deleting it is the guard's own job, and only for its own names in its own folder.
    let tmp_in_backups = "src/.lib.rs.guardtest.tmp";
    assert_eq!(TOOL.discard_interrupted_save(&dir, tmp_in_backups), Ok(()));
    assert!(!tmp.exists());
    for rel in [
        "src/lib.rs",
        "src/lib.rs.tmp",
        "../src/.lib.rs.guardtest.tmp",
    ] {
        assert_eq!(
            TOOL.discard_interrupted_save(&dir, rel),
            Err(GuardFailure::new(
                format!(
                    "refusing to delete `{rel}` under target/xtask/guarded: it is not a temporary file of the guard (.<name>.guardtest.tmp)"
                ),
                "pass the temporary file's path under the backup folder".to_owned(),
            ))
        );
    }
}

#[test]
fn a_backup_folder_that_cannot_be_made_names_the_os_error_and_edits_nothing() {
    let dir = repo("no-folder");
    // A file where the backup folder goes.
    Scratch::create_dir_all(dir.join("target/xtask")).expect("create target/xtask");
    Scratch::write(dir.join("target/xtask/guarded"), "a file").expect("block the folder");
    let error = Scratch::create_dir_all(dir.join("target/xtask/guarded/src"))
        .expect_err("the same error the guard gets");
    let Err(failure) = Guard::save(&dir, TOOL, REL, ORIGINAL) else {
        panic!("the save fails");
    };
    assert_eq!(
        failure,
        GuardFailure::new(
            format!("could not back up {REL} before editing it: {error}"),
            format!("fix what the error names, then run again; {REL} was not edited"),
        )
    );
    assert_eq!(text(&dir.join(REL)), ORIGINAL);
}

/// Makes `dir` read-only (true) or writable again (false).
#[cfg(unix)]
fn read_only(dir: &Path, on: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if on { 0o555 } else { 0o755 };
    Scratch::set_permissions(dir, fs::Permissions::from_mode(mode)).expect("chmod the folder");
}

/// What the OS says when a read-only folder refuses a new file.
#[cfg(unix)]
fn permission_denied() -> io::Error {
    io::Error::from_raw_os_error(13)
}

#[cfg(unix)]
#[test]
fn a_failed_write_leaves_the_file_unchanged_and_says_so() {
    let dir = repo("write-fails");
    let mut guard = Guard::save(&dir, TOOL, REL, ORIGINAL).expect("the save");
    read_only(&dir.join("src"), true);
    let failure = guard
        .write(EDIT.to_owned())
        .expect_err("no temporary file can be made");
    read_only(&dir.join("src"), false);
    assert_eq!(
        failure,
        GuardFailure::new(
            format!(
                "could not write {REL}: {}; the file is unchanged",
                permission_denied()
            ),
            "fix what the error names, then run again".to_owned(),
        )
    );
    assert_eq!(text(&dir.join(REL)), ORIGINAL);
    guard
        .restore()
        .expect("the original is still the file, so the copy just goes");
    assert!(!dir.join(COPY).exists());
}

#[cfg(unix)]
#[test]
fn a_failed_restore_keeps_the_edit_and_the_copy_and_says_how_to_finish() {
    let dir = repo("restore-fails");
    let mut guard = Guard::save(&dir, TOOL, REL, ORIGINAL).expect("the save");
    guard.write(EDIT.to_owned()).expect("the write");
    read_only(&dir.join("src"), true);
    let failure = guard.restore().expect_err("no temporary file can be made");
    read_only(&dir.join("src"), false);
    assert_eq!(
        failure,
        GuardFailure::new(
            format!(
                "could not restore {REL}: {}; it still holds this run's edit, and its original is in {COPY}",
                permission_denied()
            ),
            format!(
                "fix what the error names, then copy {COPY} over {REL}: the file holds nothing but \
                 this run's edit"
            ),
        )
    );
    assert_eq!(text(&dir.join(REL)), EDIT);
    assert_eq!(text(&dir.join(COPY)), ORIGINAL);
}

#[cfg(unix)]
#[test]
fn a_leftover_goes_back_through_a_temporary_file_or_stays_with_the_os_error() {
    let dir = repo("leftover");
    Scratch::create_dir_all(dir.join("target/xtask/guarded/src")).expect("create the folder");
    Scratch::write(dir.join(COPY), ORIGINAL).expect("the copy a killed run left");
    Scratch::write(dir.join(REL), EDIT).expect("the edit it left");
    let write_tmp = dir.join("src/.lib.rs.guardtest.tmp");
    Scratch::write(&write_tmp, "half an edit").expect("a killed write's temporary file");
    assert_eq!(TOOL.discard_interrupted_write(&dir, REL), Ok(()));
    assert!(!write_tmp.exists());
    assert_eq!(
        TOOL.discard_interrupted_write(&dir, REL),
        Ok(()),
        "none left is fine"
    );

    read_only(&dir.join("src"), true);
    let failure = TOOL.restore_leftover(&dir, REL);
    read_only(&dir.join("src"), false);
    assert_eq!(
        failure,
        Err(GuardFailure::new(
            format!(
                "could not restore {REL} from its copy {COPY}: {}; {REL} is unchanged",
                permission_denied()
            ),
            "fix what the error names, then run again".to_owned(),
        ))
    );
    assert_eq!(text(&dir.join(REL)), EDIT);

    assert_eq!(TOOL.restore_leftover(&dir, REL), Ok(()));
    assert_eq!(text(&dir.join(REL)), ORIGINAL);
    assert!(
        !dir.join("target/xtask/guarded/src").exists(),
        "the copy and the folders it leaves empty are gone"
    );
    assert_eq!(
        TOOL.restore_leftover(&dir, "src/.lib.rs.guardtest.tmp"),
        Err(refusal("src/.lib.rs.guardtest.tmp")),
        "a temporary file is never put back"
    );
}
