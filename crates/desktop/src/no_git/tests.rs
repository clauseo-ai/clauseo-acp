//! The app's copy of the `git` that does nothing, named to the host with its hash.

use sha2::{Digest as _, Sha256};

use crate::test_support::Scratch;

/// `clauseo-mcp` beside the scratch app's executable, as a release bundles it.
fn app(scratch: &Scratch) -> std::path::PathBuf {
    scratch.dir("app");
    let mcp = format!("app/clauseo-mcp{}", std::env::consts::EXE_SUFFIX);
    scratch.file(&mcp, b"the mcp binary");
    scratch.path().join("app").join("clauseo-desktop")
}

#[test]
fn the_host_gets_the_copy_and_the_hash_it_must_hold() {
    let scratch = Scratch::new("no-git-copy", &[]);
    let exe = app(&scratch);
    let locations = clauseo_settings::Locations::under_home(scratch.path());
    let config = super::with_no_git_from(scratch.config(Vec::new()), &locations, Some(exe));
    let no_git = config.no_git().unwrap();
    let name = format!("git{}", std::env::consts::EXE_SUFFIX);
    let program = locations.data_dir().join("tools").join("no-git").join(name);
    assert_eq!(no_git.program(), program.as_path());
    let want: [u8; 32] = Sha256::digest(b"the mcp binary").into();
    assert_eq!(no_git.sha256(), Some(want));
    assert_eq!(std::fs::read(&program).unwrap(), b"the mcp binary");
}

#[test]
fn with_no_binary_to_copy_the_host_still_gets_the_program_with_no_hash() {
    let scratch = Scratch::new("no-git-none", &[]);
    let locations = clauseo_settings::Locations::under_home(scratch.path());
    let name = format!("git{}", std::env::consts::EXE_SUFFIX);
    let program = locations.data_dir().join("tools").join("no-git").join(name);
    let exe = scratch.path().join("app").join("clauseo-desktop");
    let config = super::with_no_git_from(scratch.config(Vec::new()), &locations, Some(exe));
    let no_git = config.no_git().unwrap();
    assert_eq!(
        (no_git.program(), no_git.sha256()),
        (program.as_path(), None)
    );
    let config = super::with_no_git_from(scratch.config(Vec::new()), &locations, None);
    let no_git = config.no_git().unwrap();
    assert_eq!(
        (no_git.program(), no_git.sha256()),
        (program.as_path(), None)
    );
}
