//! The check on a copy of codex-acp 2.10.0's modes (`modes-2.10.0.js`, see `tests.rs`), and on
//! a whole fake install. The `node --check` tests skip without a `node` on `PATH`, and fail on CI.

use super::*;
use crate::write::TestDir;

/// codex-acp 2.10.0's modes, as `tests.rs` has them.
const MODES_JS: &[u8] = include_bytes!("../modes-2.10.0.js");

/// The modes as a whole file: they are class members in codex-acp's bundle, so the file is the
/// class around them, which node can parse.
fn as_file(modes: &[u8]) -> Vec<u8> {
    let mut out = b"class AgentModes {\n".to_vec();
    out.extend_from_slice(modes);
    out.extend_from_slice(b"\n}\n");
    out
}

/// The file in a fake install, `node_modules/` below the folder.
fn install(dir: &TestDir, contents: &[u8]) -> PathBuf {
    dir.file(&format!("node_modules/{IN_MODULES}"), contents);
    dir.0.join("node_modules")
}

/// Whether a `node` is on `PATH`; without one the test prints why it skips, unless CI.
fn node_present() -> bool {
    if node_on_path().is_some() {
        return true;
    }
    let on_ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
    assert!(!on_ci, "node is missing on CI");
    eprintln!("skipped: no node on PATH");
    false
}

#[test]
fn a_file_holding_the_modes_passes_the_pattern_step() {
    let patched = pattern_step(MODES_JS).unwrap();
    let text = std::str::from_utf8(&patched).unwrap();
    assert_eq!(text.matches("CLAUSEO_CODEX_NETWORK").count(), 2);
    // Already patched: the same bytes pass.
    assert_eq!(pattern_step(&patched).unwrap(), patched);
}

#[test]
fn a_mode_missing_fails_the_pattern_step_naming_it() {
    let all = std::str::from_utf8(MODES_JS).unwrap();
    let missing = all.replacen("static Agent = new", "static Agen = new", 1);
    let Err(Step::Pattern(err)) = pattern_step(missing.as_bytes()) else {
        panic!("a missing mode passes");
    };
    assert_eq!(err, PatchError::Anchor("agent"));
}

#[test]
fn a_mode_twice_fails_the_pattern_step_naming_it() {
    let all = std::str::from_utf8(MODES_JS).unwrap();
    let twice = format!("{all}{all}");
    let Err(Step::Pattern(err)) = pattern_step(twice.as_bytes()) else {
        panic!("a mode twice passes");
    };
    assert_eq!(err, PatchError::Anchor("workspace-write"));
}

#[test]
fn a_missing_file_is_a_read_step() {
    let dir = TestDir::new("codex-check-read");
    let err = check_codex_patch(&dir.0.join("node_modules")).unwrap_err();
    assert_eq!(err.step(), "read");
    assert!(err.file().ends_with(IN_MODULES), "{}", err.file().display());
}

#[test]
fn the_check_names_the_mode_and_the_file_when_a_mode_is_missing() {
    let dir = TestDir::new("codex-check-mode");
    let all = std::str::from_utf8(MODES_JS).unwrap();
    let missing = all.replacen("static WorkspaceWrite = new", "static Other = new", 1);
    let modules = install(&dir, missing.as_bytes());
    let err = check_codex_patch(&modules).unwrap_err();
    assert_eq!(err.step(), "workspace-write");
    assert!(err.file().ends_with(IN_MODULES), "{}", err.file().display());
    assert!(err.to_string().contains("Clauseo patches 2.10.0"), "{err}");
}

#[test]
fn a_real_file_of_the_modes_passes_and_leaves_no_copy_beside_it() {
    if !node_present() {
        return;
    }
    let dir = TestDir::new("codex-check-ok");
    let modules = install(&dir, &as_file(MODES_JS));
    let file = check_codex_patch(&modules).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), as_file(MODES_JS));
    let names: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.js"]);
}

#[test]
fn a_syntax_error_in_the_patched_copy_fails_node_check() {
    if !node_present() {
        return;
    }
    let dir = TestDir::new("codex-check-syntax");
    let file = dir.file("dist/index.js", b"const = ;\n");
    let Err(Step::Syntax(line)) = syntax_step(&file, b"const = ;\n") else {
        panic!("a syntax error passes node --check");
    };
    assert!(line.contains("SyntaxError"), "{line}");
    let err = CheckError::new(&file, Step::Syntax(line));
    assert_eq!(err.step(), "node --check");
    let names: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["index.js"]);
}

#[test]
fn a_patched_file_that_does_not_parse_fails_the_whole_check() {
    if !node_present() {
        return;
    }
    let dir = TestDir::new("codex-check-parse");
    let mut broken = as_file(MODES_JS);
    broken.extend_from_slice(b"function (\n");
    let modules = install(&dir, &broken);
    let err = check_codex_patch(&modules).unwrap_err();
    assert_eq!(err.step(), "node --check");
    assert!(err.file().ends_with(IN_MODULES), "{}", err.file().display());
}
