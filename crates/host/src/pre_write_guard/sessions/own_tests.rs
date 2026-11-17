//! The guard's `view_image` check in the session's own folders (`--own-folder`): its work folder
//! and its matter's notes are open to it, judged by where a path resolves; the rest of the run
//! folder, another run's, and the rest of the data folder stay refused.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::super::super::DENY;
use super::super::{DATA_IMAGE, OWN_FLAG};
use super::{Fixture, MAIN, guard};
use crate::write;

/// The session's run folder and its matter's notes in the data folder, as the host lays them out
/// (`actor/turn/launch_checks.rs`, `RunFolders`), made, with an image in the run's work folder
/// and one in its temp folder; and the hook's arguments naming its work folder and the notes as
/// the session's own, as the launch does (`actor/turn/agents/launch.rs`).
fn own_folders(fixture: &Fixture) -> (PathBuf, Vec<OsString>) {
    let run = fixture.data().join("run").join("s1");
    let work = run.join("work");
    write::create_dir_all(&work).unwrap();
    write::create_dir_all(&run.join("tmp")).unwrap();
    write::create_dir_all(&notes(fixture)).unwrap();
    write::write_file(&work.join("page-1.png"), b"x").unwrap();
    write::write_file(&run.join("tmp").join("shot.png"), b"x").unwrap();
    write::write_file(&notes(fixture).join("chart.png"), b"x").unwrap();
    (run, args_naming(fixture, &work))
}

/// The matter's notes in the fixture's data folder.
fn notes(fixture: &Fixture) -> PathBuf {
    fixture.data().join("agent-state").join("m1").join("notes")
}

/// The hook's arguments naming `work` and the matter's notes as the session's own.
fn args_naming(fixture: &Fixture, work: &Path) -> Vec<OsString> {
    let mut args = fixture.args();
    for folder in [work.to_path_buf(), notes(fixture)] {
        args.extend([OsString::from(OWN_FLAG), folder.into_os_string()]);
    }
    args
}

/// A `view_image` of `path` in the fixture's matter, from the conversation's session.
fn view(fixture: &Fixture, path: &Path) -> Vec<u8> {
    let hook = fixture.hook(
        Some(MAIN),
        "view_image",
        &json!({ "path": path.to_str().unwrap() }),
    );
    hook.to_string().into_bytes()
}

/// A `view_image` of a page rendered into the session's work folder, or of an image in its
/// matter's notes, is allowed when the hook names them as its own.
#[test]
fn view_image_in_the_sessions_own_folders_is_allowed() {
    let fixture = Fixture::new("view-own");
    let (run, args) = own_folders(&fixture);
    let notes = notes(&fixture);
    for path in [run.join("work").join("page-1.png"), notes.join("chart.png")] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        assert_eq!(
            guard(&args, hook.to_string().as_bytes()),
            (0, String::new()),
            "{path:?}"
        );
        // Without the own folders named, it is refused as before.
        let (code, err) = guard(&fixture.args(), hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// Another run's work folder, and the rest of the data folder, stay refused for a `view_image`.
#[test]
fn view_image_in_another_runs_work_folder_is_refused() {
    let fixture = Fixture::new("view-other");
    let (_, args) = own_folders(&fixture);
    let other = fixture.data().join("run").join("s2").join("work");
    write::create_dir_all(&other).unwrap();
    write::write_file(&other.join("page-1.png"), b"x").unwrap();
    for path in [other.join("page-1.png"), fixture.data().join("store.png")] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        let (code, err) = guard(&args, hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// A link in the session's work folder to elsewhere in the data folder is judged by where it
/// leads: refused.
#[cfg(unix)]
#[test]
fn view_image_through_a_link_in_the_work_folder_is_refused() {
    let fixture = Fixture::new("view-link");
    let (run, args) = own_folders(&fixture);
    let other = fixture.data().join("run").join("s2").join("work");
    write::create_dir_all(&other).unwrap();
    write::write_file(&other.join("page-1.png"), b"x").unwrap();
    let work = run.join("work");
    write::symlink(&fixture.data().join("store.png"), &work.join("store.png")).unwrap();
    write::symlink(&other, &work.join("other")).unwrap();
    for path in [
        work.join("store.png"),
        work.join("other").join("page-1.png"),
    ] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        let (code, err) = guard(&args, hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// The rest of the session's run folder, here its temp folder, is not its own: a `view_image`
/// there is refused.
#[test]
fn view_image_elsewhere_in_the_run_folder_is_refused() {
    let fixture = Fixture::new("view-run-rest");
    let (run, args) = own_folders(&fixture);
    for path in [run.join("tmp").join("shot.png"), run.join("page.png")] {
        let (code, err) = guard(&args, &view(&fixture, &path));
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// Another spelling of an own folder names the same folder, as the volume reads it: on macOS and
/// Windows, whose volumes ignore case, the folder named in other case (and the image too); on
/// Windows also with forward slashes.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn another_spelling_of_an_own_folder_is_the_same_folder() {
    let fixture = Fixture::new("view-spelling");
    let (run, _) = own_folders(&fixture);
    let work = run.join("work");
    let upper = fixture.data().join("RUN").join("S1").join("WORK");
    // Forward slashes after the data folder: on Windows a mixed spelling, elsewhere the plain one.
    let slashed = PathBuf::from(format!("{}/run/s1/work", fixture.data().display()));
    for spelling in [upper.clone(), slashed] {
        let args = args_naming(&fixture, &spelling);
        for image in [work.join("page-1.png"), upper.join("PAGE-1.PNG")] {
            assert_eq!(
                guard(&args, &view(&fixture, &image)),
                (0, String::new()),
                "{spelling:?} {image:?}"
            );
        }
    }
}
