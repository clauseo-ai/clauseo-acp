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
        dependencies: Default::default(),
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
        dependencies: Default::default(),
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
    let workspace = crate::workspace::load(&root).expect("cargo metadata runs");
    let ctx = Context {
        members: workspace.members,
        dependencies: workspace.dependencies,
        load_resolved_graph: false,
        root,
    };
    for check in ALL {
        let report = check.report(&ctx);
        assert!(!report.failed(), "{}: {:?}", check.name, report.printed());
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
        dependencies: Default::default(),
        load_resolved_graph: false,
    };
    for name in STOPS_IN_AN_EMPTY_REPO {
        assert!(
            ALL.iter().any(|check| check.name == name),
            "no check {name}"
        );
    }
    for check in ALL {
        let report = check.report(&ctx);
        assert_eq!((check.name, report.inspection.inspected), (check.name, 0));
        let fix = report.nothing_inspected.map(|failure| failure.fix);
        if STOPS_IN_AN_EMPTY_REPO.contains(&check.name) {
            assert_eq!(
                (check.name, fix.as_deref()),
                (check.name, Some(STOPPED_FIX))
            );
        } else {
            let normal = format!(
                "{} looks for {} {}; restore them, or, if they moved, update the consts {} builds \
                 that place from",
                check.name,
                check.counts.many,
                (check.place)(),
                check.source
            );
            assert_eq!((check.name, fix), (check.name, Some(normal)));
        }
    }
}

/// The checks that stop before they look in an empty folder with no workspace members: the
/// desktop checks find no desktop crate, and docs and file-size cannot list files without git.
/// The others look and find nothing: budgets reads no budgets.toml, buildtime-edits no
/// ui/app/src, dependencies and core-io no Crates table.
const STOPS_IN_AN_EMPTY_REPO: [&str; 5] = [
    "capabilities",
    "plugins",
    "tauri-config",
    "docs",
    "file-size",
];

#[test]
fn a_violation_with_nothing_inspected_gets_the_stopped_fix_only_when_the_check_stopped() {
    let gone = || violation("a.json", 1, "gone", "restore it");
    let check = fake(|_| Inspection::default());
    let looked = check.judge(Inspection::new(0, vec![gone()]));
    assert_eq!(
        looked.nothing_inspected.map(|failure| failure.fix),
        Some(
            "tauri-config looks for capability files in the desktop crate's capabilities/; \
             restore them, or, if they moved, update the consts xtask/src/checks/tauri_config.rs \
             builds that place from"
                .to_owned()
        )
    );
    let stopped = check.judge(Inspection::stopped(gone()));
    assert_eq!(
        stopped.nothing_inspected.map(|failure| failure.fix),
        Some(STOPPED_FIX.to_owned())
    );
    let silent = check.judge(Inspection::stopped_on(Vec::new()));
    assert_ne!(
        silent.nothing_inspected.map(|failure| failure.fix),
        Some(STOPPED_FIX.to_owned()),
        "a stop with nothing printed above must not send the reader above"
    );
}

/// No two entries in `ALL` share a place, so a check copied from another one that keeps its
/// `place` (say, built from `budgets::FILE`) fails here.
#[test]
fn no_two_checks_share_a_place() {
    let mut places = std::collections::BTreeMap::new();
    for check in ALL {
        if let Some(other) = places.insert((check.place)(), check.name) {
            panic!(
                "{} and {other} share the place `{}`",
                check.name,
                (check.place)()
            );
        }
    }
}

/// Every entry in `ALL` is a whole check: a unique name, the file it lives in, a place, and
/// planted cases that include a pass and a failure. A check left out of `ALL` is caught by the
/// compiler instead: nothing outside tests reads its `CHECK`, so clippy's dead-code lint fails.
#[test]
fn every_check_in_all_has_a_unique_name_its_file_a_place_and_planted_cases() {
    let root = crate::workspace::workspace_root();
    let mut names = std::collections::BTreeSet::new();
    for check in ALL {
        assert!(names.insert(check.name), "{} is in ALL twice", check.name);
        assert!(
            check.source.starts_with("xtask/src/checks/") && root.join(check.source).is_file(),
            "{} names {}, not its file under xtask/src/checks/",
            check.name,
            check.source
        );
        assert!(!(check.place)().is_empty(), "{} has no place", check.name);
        let expects: Vec<planted::Expect> = check.planted.iter().map(|case| case.expect).collect();
        assert!(
            expects.contains(&planted::Expect::Passes),
            "{} has no planted case that passes",
            check.name
        );
        assert!(
            expects
                .iter()
                .any(|expect| matches!(expect, planted::Expect::FailsWith(_))),
            "{} has no planted case that fails",
            check.name
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
            fix: "tauri-config looks for capability files in the desktop crate's capabilities/; restore them, or, if they moved, update the consts xtask/src/checks/tauri_config.rs builds that place from".to_owned(),
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

/// A failure in `inspection.failures` fails the report even when nothing is a violation and
/// something was inspected, so a check that reports a failure after looking still fails.
#[test]
fn a_failure_with_no_violation_and_something_inspected_fails_the_report() {
    let check = fake(|_| Inspection {
        inspected: 1,
        failures: vec![Failure {
            message: "could not list the repo's files".to_owned(),
            fix: "install git".to_owned(),
        }],
        ..Inspection::default()
    });
    let report = check.report(&desktop_context(Path::new("/ws")));
    assert_eq!(report.inspection.violations, Vec::new());
    assert_eq!(report.nothing_inspected, None);
    assert!(report.failed());
}

#[test]
fn a_git_that_cannot_start_says_so_and_tells_to_install_it() {
    let error = GitError::NotStarted("No such file or directory (os error 2)".to_owned());
    let report = fake(|_| Inspection::default()).judge(unlisted("file-size", &error));
    assert_eq!(
        report.printed(),
        [
            "could not list the repo's files: could not run git: No such file or directory (os \
             error 2)\n    \
             fix: install git and put it on PATH: file-size lists the files it reads with \
             `git ls-files`",
            "0 capability files inspected in the desktop crate's capabilities/\n    \
             fix: the check stopped before it could look; fix the failure above first",
        ]
    );
}

#[test]
fn an_unreadable_path_asks_for_utf8_only_when_that_is_the_error() {
    let not_text = io::Error::new(
        io::ErrorKind::InvalidData,
        "stream did not contain valid UTF-8",
    );
    assert_eq!(
        unreadable("a.rs", &not_text).to_string(),
        "a.rs:1: could not read `a.rs`: stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text"
    );
    let gone = io::Error::new(io::ErrorKind::NotFound, "No such file or directory");
    assert_eq!(
        unreadable("src", &gone).to_string(),
        "src:1: could not read `src`: No such file or directory\n    \
         fix: fix what the error says so the check can read it: restore or replace the file or \
         folder"
    );
}

#[test]
fn line_numbers_start_at_one() {
    assert_eq!(line_at(0), 1);
    assert_eq!(line_at(4), 5);
}
