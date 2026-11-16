//! The pre-write guard's launch self-test against the built app, the real guard program, started
//! as Claude Code starts its hook (`clauseo_host::pre_write_guard::selftest_timed_for_test`, the
//! same path as each Claude launch's self-test). It must pass, and it prints how long each run
//! took, so CI logs the figure on every OS: the first run is the cold start (the one Windows
//! Defender may scan), the next ones warm. `guard_selftest::LIMIT` and the guard's watchdog
//! deadline are to be set from these figures.

use std::path::Path;

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");

#[test]
fn the_built_guard_passes_its_launch_self_test_and_the_time_is_logged() {
    let matter = tempfile::Builder::new()
        .prefix("guard-selftest-")
        .tempdir()
        .unwrap();
    for (run, forced_ask) in [("cold", false), ("warm", false), ("warm, forced ask", true)] {
        let (result, took) = clauseo_host::pre_write_guard::selftest_timed_for_test(
            Path::new(APP),
            matter.path(),
            forced_ask,
        );
        eprintln!(
            "guard self-test ({run}, {}): {} ms, {result:?}",
            std::env::consts::OS,
            took.as_millis()
        );
        assert_eq!(result, Ok(()), "{run}");
    }
}
