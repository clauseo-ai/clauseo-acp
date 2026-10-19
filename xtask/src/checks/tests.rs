//! Tests of the shared helpers and of the rules every check meets.

use super::*;
use crate::scratch::Scratch;

#[test]
fn a_missing_file_reads_as_none_and_a_present_one_as_its_text() {
    let dir = Scratch::new("checks-read");
    assert_eq!(read_repo_file(&dir, "a.txt"), Ok(None));
    fs::write(dir.join("a.txt"), "hello").expect("write the file");
    assert_eq!(read_repo_file(&dir, "a.txt"), Ok(Some("hello".to_owned())));
}

#[test]
fn a_path_is_repo_relative_with_slashes() {
    let root = Path::new("/ws");
    assert_eq!(
        repo_path(root, Path::new("/ws/crates/desktop/x.json")),
        "crates/desktop/x.json"
    );
}

/// A context whose members include the desktop crate at `crates/desktop`.
pub(crate) fn desktop_context(root: &Path) -> Context {
    Context {
        root: root.to_path_buf(),
        members: vec![crate::workspace::Member {
            name: DESKTOP_PACKAGE.to_owned(),
            dir: "crates/desktop".to_owned(),
            path_deps: Vec::new(),
        }],
        load_resolved_graph: false,
    }
}

#[test]
fn the_desktop_dir_comes_from_the_member_with_its_package_name() {
    let mut ctx = desktop_context(Path::new("/ws"));
    ctx.members[0].dir = "apps/shell".to_owned();
    assert_eq!(desktop_dir(&ctx), Ok("apps/shell"));
}

#[test]
fn a_missing_desktop_member_is_one_violation_at_the_root_manifest() {
    let ctx = Context {
        root: Path::new("/ws").to_path_buf(),
        members: Vec::new(),
        load_resolved_graph: false,
    };
    assert_eq!(
        desktop_dir(&ctx),
        Err(violation(
            "Cargo.toml",
            1,
            "the desktop crate (package `clauseo-desktop`) is not a workspace member",
            "restore crates/desktop or update xtask/src/checks: these checks guard its capabilities, plugins and Tauri config",
        ))
    );
}

#[test]
fn the_real_repo_passes_every_static_check() {
    let root = crate::workspace::workspace_root();
    let ctx = Context {
        members: crate::workspace::load_members(&root).expect("cargo metadata runs"),
        load_resolved_graph: false,
        root,
    };
    for check in ALL {
        let report = check.report(&ctx);
        assert_eq!(
            (
                check.name,
                &report.inspection.violations,
                &report.nothing_inspected
            ),
            (check.name, &Vec::new(), &None)
        );
        assert!(
            report.inspection.inspected > 0,
            "{} inspected nothing",
            check.name
        );
    }
}

#[test]
fn every_check_inspects_nothing_in_an_empty_repo() {
    let dir = Scratch::new("checks-empty-repo");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
        load_resolved_graph: false,
    };
    for check in ALL {
        let report = check.report(&ctx);
        assert_eq!((check.name, report.inspection.inspected), (check.name, 0));
        let stopped =
            !report.inspection.violations.is_empty() || !report.inspection.failures.is_empty();
        assert_eq!(
            (check.name, report.nothing_inspected),
            (check.name, Some(check.nothing_inspected(stopped)))
        );
    }
}

#[test]
fn every_check_names_a_source_file_that_exists() {
    let root = crate::workspace::workspace_root();
    for check in ALL {
        assert!(
            root.join(check.source).is_file(),
            "{} has no {}",
            check.name,
            check.source
        );
    }
}

/// A check named `tauri-config` that counts capability files and returns what `run` returns.
fn fake(run: fn(&Context) -> Inspection) -> StaticCheck {
    StaticCheck::fake(
        "tauri-config",
        Noun {
            one: "capability file",
            many: "capability files",
        },
        || "in the desktop crate's capabilities/".to_owned(),
        "xtask/src/checks/tauri_config.rs",
        run,
    )
}

#[test]
fn zero_inspected_fails_without_a_location_and_names_where_the_check_looked() {
    let check = fake(|_| Inspection::default());
    let report = check.report(&desktop_context(Path::new("/ws")));
    assert!(report.failed());
    assert_eq!(report.counted, "0 capability files inspected");
    assert_eq!(
        report.nothing_inspected,
        Some(Failure {
            message: "0 capability files inspected in the desktop crate's capabilities/".to_owned(),
            fix: "tauri-config looks for capability files in the desktop crate's capabilities/; restore them, or update xtask/src/checks/tauri_config.rs if they moved".to_owned(),
        })
    );
}

#[test]
fn a_count_above_zero_passes_unless_there_are_violations_and_notes_never_fail() {
    let check = fake(|_| Inspection {
        inspected: 1,
        notes: vec!["a.rs has 550 of 600 lines; split it now".to_owned()],
        ..Inspection::default()
    });
    let report = check.report(&desktop_context(Path::new("/ws")));
    assert!(!report.failed());
    assert_eq!(report.counted, "1 capability file inspected");

    let check = fake(|_| Inspection::new(2, vec![violation("a.json", 3, "bad", "fix it")]));
    let report = check.report(&desktop_context(Path::new("/ws")));
    assert!(report.failed());
    assert_eq!(report.nothing_inspected, None);
    assert_eq!(report.counted, "2 capability files inspected");
}

#[test]
fn line_numbers_start_at_one() {
    assert_eq!(line_at(0), 1);
    assert_eq!(line_at(4), 5);
}
