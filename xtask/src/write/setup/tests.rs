//! Tests for setup's writes: each stays in bin and the staging dir it made.

use super::*;
use crate::scratch::Scratch;

#[test]
fn a_staging_dir_is_one_name_in_bin_and_installs_only_its_own_files() {
    let dir = Scratch::new("write-setup-staging");
    let bin = dir.join("bin");
    let refused = Staging::fresh(&bin, "../escape").expect_err("a path is no label");
    assert_eq!(
        refused.to_string(),
        "`../escape` is not a staging label: it must be one plain file name"
    );
    assert!(!bin.exists(), "nothing was made");

    let staging = Staging::fresh(&bin, "tool-1").expect("a fresh staging dir");
    assert_eq!(staging.dir(), bin.join(".xtask-setup-tool-1"));
    Scratch::write(staging.dir().join("tool"), "binary").expect("a download");
    Scratch::write(dir.join("elsewhere"), "binary").expect("a file outside");
    let outside = staging
        .install(&dir.join("elsewhere"), "tool")
        .expect_err("only a file in the staging dir is installed");
    assert_eq!(
        outside.to_string(),
        format!(
            "{} is not directly in the staging dir {}, so it is not installed",
            dir.join("elsewhere").display(),
            staging.dir().display()
        )
    );
    let named = staging
        .install(&staging.dir().join("tool"), "../tool")
        .expect_err("a path is no binary name");
    assert_eq!(
        named.to_string(),
        "`../tool` is not a binary name: it must be one plain file name"
    );
    staging
        .install(&staging.dir().join("tool"), "tool")
        .expect("the install");
    assert_eq!(
        fs::read_to_string(bin.join("tool")).expect("installed"),
        "binary"
    );
    staging.remove().expect("the staging dir goes");
    assert!(!staging.dir().exists());
    staging
        .remove()
        .expect("a staging dir already gone is fine");
}

#[test]
fn the_run_lock_is_created_when_missing_and_never_truncated() {
    let dir = Scratch::new("write-setup-run-lock");
    let bin = dir.join("bin");
    let lock = open_run_lock(&bin).expect("created with bin");
    lock.try_lock().expect("the lock is free");
    drop(lock);
    Scratch::write(bin.join(RUN_LOCK), "left by an earlier run").expect("some bytes");
    drop(open_run_lock(&bin).expect("opened"));
    assert_eq!(
        fs::read_to_string(bin.join(RUN_LOCK)).expect("read it"),
        "left by an earlier run"
    );
}
