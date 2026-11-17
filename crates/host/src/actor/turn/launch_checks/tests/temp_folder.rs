//! The session's temp folder with its run folder: made with it, and gone with it. Split from
//! `tests.rs` for its size.

use super::{RunSweep, folders, scratch};
use crate::write;

/// The session's temp folder (D-023) is made with its run folder and goes with it.
#[test]
fn the_temp_folder_is_made_in_the_run_folder_and_goes_with_it() {
    let data = scratch("temp-folder");
    let launch = folders(&data, 1);
    launch.make(&data, &RunSweep::default()).unwrap();
    let tmp = crate::agent::temp::temp_dir(&launch.root);
    assert_eq!(tmp, data.join("run").join(launch.key.id()).join("tmp"));
    assert!(tmp.is_dir());
    write::write_file(&tmp.join("module.pcm"), b"cache").unwrap();
    launch.remove();
    assert!(
        !launch.root.exists(),
        "the temp folder went with the run folder"
    );
    write::remove_dir_all(&data).unwrap();
}
