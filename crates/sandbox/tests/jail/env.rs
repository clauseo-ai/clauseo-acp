//! The jail's environment is the spec's plus `TMPDIR`: nothing of the host's reaches it.

use crate::support::{World, launcher, run};

#[test]
fn the_jail_gets_only_the_specs_variables_and_tmpdir() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let tmpdir = policy.tmpdir();
    let (launcher, _) = launcher(policy);
    let out = run(
        &launcher,
        "/usr/bin/env",
        &[],
        &w.at("m1"),
        &[("CLAUSEO_PROBE", "1")],
    );
    let mut lines: Vec<&str> = out.assert_ok().stdout.lines().collect();
    lines.sort_unstable();
    let tmp = format!("TMPDIR={}", tmpdir.display());
    assert_eq!(lines, ["CLAUSEO_PROBE=1", tmp.as_str()], "{out:#?}");
}
