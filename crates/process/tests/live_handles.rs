//! [`clauseo_process::live_handles`] against `fake_agent`'s `idle` scenario: the count rises
//! with a start and falls when its handle is dropped. One test in this binary, so no other
//! start moves the count.

use clauseo_process::{LaunchSpec, Launched, SpawnOptions, live_handles, spawn_in_tree};

/// A start counts until its handle is dropped (the drop kills the tree), whether or not the
/// agent has exited; nothing started counts as zero.
#[test]
fn a_start_counts_until_its_handle_is_dropped() {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    let spec = LaunchSpec::new(
        bin,
        vec!["--scenario".into(), "idle".into()],
        std::collections::BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    assert_eq!(live_handles(), 0);
    let Launched {
        stdin,
        stdout,
        stderr,
        handle,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    assert_eq!(live_handles(), 1);
    // The pipes go first: only the handle counts.
    drop((stdin, stdout, stderr));
    assert_eq!(live_handles(), 1);
    drop(handle);
    assert_eq!(live_handles(), 0);
}
