//! The jail's environment is the spec's plus `TMPDIR`: nothing of the host's reaches it.

use crate::support::{World, launcher, run, sh};

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

/// The session's temp folder the host names, `data/run/s1/tmp`: the jail's `TMPDIR`, made by
/// the launcher, and writable; the run folder that holds it (and its token) is not.
#[test]
fn the_session_tmpdir_is_writable_and_its_run_folder_is_not() {
    let w = World::new();
    w.file("data/run/s1/token", "secret token\n");
    let policy = w.builder().tmpdir(w.at("data/run/s1/tmp")).build().unwrap();
    let tmpdir = policy.tmpdir();
    let run = tmpdir.parent().unwrap().to_owned();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let out = sh(
        &launcher,
        &m1,
        r#"printf x > "$TMPDIR/scratch" && echo "$TMPDIR""#,
        &[],
    );
    assert_eq!(
        out.assert_ok().stdout.trim(),
        tmpdir.to_str().unwrap(),
        "{out:#?}"
    );
    assert_eq!(
        std::fs::read_to_string(tmpdir.join("scratch")).unwrap(),
        "x"
    );
    let beside = run.join("beside");
    let beside = beside.to_str().unwrap();
    sh(&launcher, &m1, r#"printf x > "$1""#, &[beside]).assert_refused();
    assert!(!run.join("beside").exists());
    let token = run.join("token");
    let token = token.to_str().unwrap();
    sh(&launcher, &m1, r#"printf x >> "$1""#, &[token]).assert_refused();
    sh(&launcher, &m1, r#"cat "$1""#, &[token]).assert_refused();
    assert_eq!(
        std::fs::read_to_string(run.join("token")).unwrap(),
        "secret token\n"
    );
}
