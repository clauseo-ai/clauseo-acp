//! A panic in the guard's process (`process.rs`) refuses: exit [`DENY`] with the plain line on
//! stderr. The test runs this test binary again with one test, which sets the guard's panic hook
//! and panics; the hook ends that process, so the panic never reaches the harness.

use std::process::Command;

use super::super::process::refuse_on_panic;
use super::super::{COULD_NOT_CHECK_AGENT, DENY};

/// The variable that makes [`panicking_guard`] set the hook and panic; unset, it does nothing.
const CHILD: &str = "CLAUSEO_GUARD_PANIC_CHILD";

/// The child: only when [`CHILD`] is set.
#[test]
fn panicking_guard() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    refuse_on_panic();
    std::panic::panic_any("an internal error in the guard");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test binary runs itself as the panicking guard"
)]
fn a_panic_in_the_guard_exits_2_with_the_plain_line() {
    let name = format!(
        "{}::panicking_guard",
        module_path!().split_once("::").unwrap().1
    );
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture", "--test-threads", "1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(DENY), "{stderr}");
    assert!(stderr.contains(COULD_NOT_CHECK_AGENT), "{stderr}");
    assert!(!stderr.contains("an internal error"), "{stderr}");
}
