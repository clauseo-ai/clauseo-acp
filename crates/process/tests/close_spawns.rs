//! [`clauseo_process::close_spawns`]: once closed, every start is refused with
//! `Error::Closing` before any command is built, and the count is left as it was. A binary of its
//! own: the gate never reopens.

use clauseo_process::{Error, LaunchSpec, SpawnOptions, close_spawns, live_handles, spawn_in_tree};

/// After the gate closes, a start is refused and starts nothing: the program does not exist, so
/// any attempt to run it would fail as `Error::Spawn`; `Error::Closing` says none was made. The
/// refused start's count is given back.
#[test]
fn a_start_after_the_gate_closes_is_refused_and_starts_nothing() {
    let missing = std::env::temp_dir().join("clauseo-close-spawns-no-such-program");
    assert!(!missing.exists());
    let spec = LaunchSpec::new(
        missing,
        Vec::new(),
        std::collections::BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    // Open: the start is tried, and fails as a spawn.
    let open = spawn_in_tree(&spec, &SpawnOptions::new()).err();
    assert!(matches!(open, Some(Error::Spawn { .. })), "{open:?}");
    assert_eq!(live_handles(), 0, "a failed start gives its count back");
    close_spawns();
    let closed = spawn_in_tree(&spec, &SpawnOptions::new()).err();
    assert!(matches!(closed, Some(Error::Closing)), "{closed:?}");
    assert_eq!(live_handles(), 0);
    // Closing again changes nothing.
    close_spawns();
    assert!(matches!(
        spawn_in_tree(&spec, &SpawnOptions::new()),
        Err(Error::Closing)
    ));
}
