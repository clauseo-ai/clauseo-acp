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
fn autoplay_is_on_only_in_the_marks_build() {
    let script = init_script();
    let expected = format!("autoplay: {},", cfg!(feature = "marks"));
    assert!(script.contains(&expected), "{script}");
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
