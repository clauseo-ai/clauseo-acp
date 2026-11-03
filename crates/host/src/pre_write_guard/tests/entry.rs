//! The binaries' entry ([`main_entry`]) and the hook's program ([`guard_program`]). The guard
//! run itself through a real process is in `crates/guard/tests/`.

use std::ffi::OsString;
use std::path::PathBuf;

use super::super::{SIDECAR, guard_program, main_entry};
use crate::write;

#[test]
fn only_the_exact_flag_first_is_a_guard_call() {
    for args in [
        &[][..],
        &["--pre-write-guard=1"],
        &["--Pre-Write-Guard"],
        &[" --pre-write-guard"],
        &["pre-write-guard"],
        &[""],
        &["clauseo://open"],
        &["--matter", "/m", "--pre-write-guard"],
    ] {
        let args = args.iter().map(OsString::from);
        assert_eq!(main_entry(args), None);
    }
}

/// A folder holding an app executable `app`, removed on drop.
struct AppFolder(PathBuf);

impl AppFolder {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "clauseo-host-sidecar-{test}-{}",
            std::process::id()
        ));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        write::write_file(&root.join("app"), b"x").unwrap();
        Self(root)
    }

    fn exe(&self) -> PathBuf {
        self.0.join("app")
    }

    fn sidecar(&self) -> PathBuf {
        self.0
            .join(format!("{SIDECAR}{}", std::env::consts::EXE_SUFFIX))
    }
}

impl Drop for AppFolder {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

#[test]
fn the_sidecar_beside_the_app_is_the_hook_program() {
    let app = AppFolder::new("present");
    write::write_file(&app.sidecar(), b"x").unwrap();
    assert_eq!(guard_program(&app.exe()), app.sidecar());
}

#[test]
fn without_the_sidecar_the_app_itself_is_the_hook_program() {
    let app = AppFolder::new("absent");
    assert_eq!(guard_program(&app.exe()), app.exe());
    // A folder of that name is no program.
    write::create_dir_all(&app.sidecar()).unwrap();
    assert_eq!(guard_program(&app.exe()), app.exe());
}
