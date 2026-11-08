//! The `TaskCache\Tree` `Id` on a real machine (`replaced.rs`, `untrusted.rs`): a task deleted
//! and registered again under the same name gets a new `Id`, so an `Id` that reads the same
//! is the same task. Gated like the tests above.

use std::process::Command;

use super::super::windows_listing::task_cache_id;
use super::{Made, gate, random_hex, system32};

/// Registers the task `name` in the root folder as this administrator, replacing one there.
#[expect(
    clippy::disallowed_methods,
    reason = "makes the test's own task through schtasks, as this process"
)]
fn register(name: &str) {
    let action = format!(r"{}\cmd.exe /c exit 0", system32());
    let output = Command::new(format!(r"{}\schtasks.exe", system32()))
        .args(["/create", "/tn", name, "/tr", &action])
        .args(["/sc", "once", "/st", "23:59", "/f"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Deletes the task `name` as this administrator.
#[expect(
    clippy::disallowed_methods,
    reason = "deletes the test's own task through schtasks, as this process"
)]
fn delete(name: &str) {
    let output = Command::new(format!(r"{}\schtasks.exe", system32()))
        .args(["/delete", "/tn", name, "/f"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_re_created_task_gets_a_new_cache_id() {
    if !gate("elevated_a_re_created_task_gets_a_new_cache_id") {
        return;
    }
    let name = format!("ClauseoSweepId{}{}", std::process::id(), random_hex(2));
    let _made = Made(vec![name.clone()]);
    let path = format!(r"\{name}");
    register(&name);
    let first = task_cache_id(&path)
        .unwrap()
        .expect("the task's cache key is there");
    delete(&name);
    assert_eq!(
        task_cache_id(&path).unwrap(),
        None,
        "the task's cache key is gone"
    );
    register(&name);
    let second = task_cache_id(&path)
        .unwrap()
        .expect("the new task's cache key is there");
    eprintln!("{name}: {first} then {second}");
    assert_ne!(
        first, second,
        "a task registered again under its name kept its Id"
    );
}
