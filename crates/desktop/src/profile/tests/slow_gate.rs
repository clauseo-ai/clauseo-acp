//! The `stetdoc-slow-test` build never ships, as `e2e`'s test key never does
//! (`update/tests/test_key.rs`): by text, release.yml builds no feature for a v* release and
//! fails when the crate's default features reach this one, and it scans every app binary it
//! builds for this build's variable and marker; by bytes, a test binary without the feature
//! holds neither, and one with it holds the marker, so the scan finds what it looks for. The
//! variable and the marker are read from `slow.rs` at run time, so this file's code holds
//! neither.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// The feature's name.
const FEATURE: &str = "stetdoc-slow-test";

/// The variable the build reads and its marker, as `slow.rs` writes them.
fn forms() -> [String; 2] {
    let source = read("src/profile/stetdoc/slow.rs");
    let quoted = |start: &str| {
        let line = source
            .lines()
            .find(|line| line.starts_with(start))
            .unwrap_or_else(|| panic!("slow.rs has no `{start}`"));
        let (open, close) = (line.find('"').unwrap(), line.rfind('"').unwrap());
        line[open + 1..close].to_owned()
    };
    [
        quoted("const SLOW_TRIES_VAR"),
        quoted("static STETDOC_SLOW_MARKER"),
    ]
}

/// The release workflow's step named `name`: its lines up to the next blank line.
fn step(workflow: &str, name: &str) -> String {
    let lines: Vec<&str> = workflow.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == format!("- name: {name}"))
        .unwrap_or_else(|| panic!("release.yml has no step {name:?}"));
    lines[at..]
        .iter()
        .take_while(|line| !line.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// A release can't turn the feature on: a v* release builds with no feature, a test release
/// with `e2e` alone, and the check fails when the crate's default features reach this one
/// (release.yml and its local copy, local-mac-dmg.sh); no default turns it on now; and every
/// run, test runs too, scans its app binary for the variable and the marker.
#[test]
fn a_release_build_can_t_turn_on_the_slow_stetdoc_test() {
    let release = read("../../.github/workflows/release.yml");
    assert!(release.contains(r#"fail "a v* release would build with --features $features""#));
    let features: Vec<&str> = release
        .lines()
        .filter(|line| line.contains("features=") && line.contains("mode="))
        .collect();
    assert_eq!(
        features.len(),
        1,
        "one tag mode sets features: {features:?}"
    );
    assert!(features[0].trim_end().ends_with("features=e2e ;;"));
    let gated = release
        .split("print(\" \".join(sorted(seen & {")
        .nth(1)
        .and_then(|rest| rest.split("})))").next())
        .expect("release.yml's check of the default features");
    assert!(gated.contains(&format!("\"{FEATURE}\"")), "{gated}");
    let local = read("../../.github/scripts/local-mac-dmg.sh");
    assert!(local.contains(&format!(",\"{FEATURE}\")))")));

    let manifest: toml::Table = toml::from_str(&read("Cargo.toml")).unwrap();
    let table = manifest["features"].as_table().unwrap();
    assert_eq!(table[FEATURE].as_array().map(Vec::len), Some(0));
    let mut todo: Vec<String> = table
        .get("default")
        .and_then(toml::Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut seen = Vec::new();
    while let Some(name) = todo.pop() {
        if !seen.contains(&name) {
            if let Some(next) = table.get(&name).and_then(toml::Value::as_array) {
                todo.extend(
                    next.iter()
                        .filter_map(|name| name.as_str().map(str::to_owned)),
                );
            }
            seen.push(name);
        }
    }
    assert!(
        !seen.iter().any(|name| name == FEATURE),
        "default reaches {FEATURE}"
    );

    let scan = step(&release, "The app binary holds no stetdoc-slow-test string");
    assert!(!scan.contains("if:"), "the scan runs in every run:\n{scan}");
    for form in forms() {
        assert!(
            scan.lines()
                .any(|line| line.trim_start().starts_with("for form in") && line.contains(&form)),
            "the scan looks for {form}:\n{scan}"
        );
    }
    assert!(scan.contains("::error::$exe holds the stetdoc-slow-test string $form"));
}

/// By the bytes built: this test binary, without the feature, holds neither the variable nor
/// the marker.
#[cfg(not(feature = "stetdoc-slow-test"))]
#[test]
fn a_build_without_the_feature_holds_no_slow_stetdoc_string() {
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    for form in forms() {
        assert_eq!(
            memchr::memmem::find(&binary, form.as_bytes()),
            None,
            "the test binary holds {form}"
        );
    }
}

/// The scan finds what it looks for: the feature's build holds the marker and the variable.
#[cfg(feature = "stetdoc-slow-test")]
#[test]
fn the_slow_stetdoc_build_holds_its_marker() {
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    for form in forms() {
        assert!(
            memchr::memmem::find(&binary, form.as_bytes()).is_some(),
            "the test binary holds no {form}"
        );
    }
}
