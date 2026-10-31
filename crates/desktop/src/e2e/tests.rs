use super::*;

#[test]
fn the_init_script_sets_the_perf_globals_before_the_react_hook() {
    let script = init_script_with(1_730_380_000_123.456, true);
    assert!(
        script.starts_with(
            "window.__CLAUSEO_PERF__ = { t0EpochMs: 1730380000123.456, autoplay: true, \
             reactCommits: 0 };\n"
        ),
        "{script}"
    );
    let perf = script.find("__CLAUSEO_PERF__").unwrap();
    let hook = script.find("__REACT_DEVTOOLS_GLOBAL_HOOK__").unwrap();
    assert!(perf < hook);
    // Only when the page has none.
    assert!(script.contains("if (window.__REACT_DEVTOOLS_GLOBAL_HOOK__ === undefined)"));
    for member in [
        "supportsFiber: true",
        "renderers: new Map()",
        "inject() { return 1; }",
        "onCommitFiberRoot() { window.__CLAUSEO_PERF__.reactCommits++; }",
        "onCommitFiberUnmount() {}",
        "onPostCommitFiberRoot() {}",
        "checkDCE() {}",
    ] {
        assert!(script.contains(member), "{member}");
    }
    assert!(init_script_with(5.0, false).contains("t0EpochMs: 5, autoplay: false,"));
    assert!(init_script_with(f64::NAN, false).contains("t0EpochMs: 0,"));
}

#[test]
fn autoplay_needs_marks_and_both_variables_set() {
    let dir = Some(OsStr::new("/tmp/matter"));
    let out = Some(OsStr::new("/tmp/run.json"));
    let empty = Some(OsStr::new(""));
    assert!(autoplay_with(true, out, dir));
    // Not a marks build.
    assert!(!autoplay_with(false, out, dir));
    // A marks build started by hand: a variable unset or empty.
    for (marks_out, matter_dir) in [
        (None, dir),
        (out, None),
        (None, None),
        (empty, dir),
        (out, empty),
        (empty, empty),
    ] {
        assert!(
            !autoplay_with(true, marks_out, matter_dir),
            "{marks_out:?} {matter_dir:?}"
        );
    }
}

#[test]
fn autoplay_is_off_outside_the_marks_build() {
    let script = init_script();
    // The test run sets neither MARKS_OUT nor E2E_MATTER_DIR unless a harness does, and a
    // marks build plays only with both.
    let expected = autoplay_with(
        cfg!(feature = "marks"),
        std::env::var_os(MARKS_OUT_VAR).as_deref(),
        std::env::var_os(MATTER_DIR_VAR).as_deref(),
    );
    assert!(
        script.contains(&format!("autoplay: {expected},")),
        "{script}"
    );
    if !cfg!(feature = "marks") {
        assert!(script.contains("autoplay: false,"), "{script}");
    }
}

#[test]
fn t0_is_the_first_mark_and_stays() {
    mark_start();
    let t0 = *T0_EPOCH_MS.get().unwrap();
    mark_start();
    assert_eq!(T0_EPOCH_MS.get().copied(), Some(t0));
    assert!(t0 > 1_700_000_000_000.0 && t0 <= epoch_ms(), "{t0}");
    assert!(init_script().contains(&format!("t0EpochMs: {t0},")));
}
