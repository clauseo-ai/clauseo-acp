//! Tests of the shared helpers and of the rules every check meets.

use super::*;
use crate::scratch::Scratch;

#[test]
fn a_missing_file_reads_as_none_and_a_present_one_as_its_text() {
    let dir = Scratch::new("checks-read");
    assert_eq!(read_repo_file(&dir, "a.txt"), Ok(None));
    Scratch::write(dir.join("a.txt"), "hello").expect("write the file");
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
        members: Ok(vec![crate::workspace::Member {
            name: DESKTOP_PACKAGE.to_owned(),
            dir: "crates/desktop".to_owned(),
            path_deps: Vec::new(),
            targets: Vec::new(),
            dependencies: crate::workspace::PackageDependencies::default(),
        }]),
        load_resolved_graph: false,
    }
}

#[test]
fn the_desktop_dir_comes_from_the_member_with_its_package_name() {
    let mut ctx = desktop_context(Path::new("/ws"));
    ctx.members.as_mut().expect("the context has members")[0].dir = "apps/shell".to_owned();
    assert_eq!(desktop_dir(&ctx), Ok("apps/shell"));
}

#[test]
fn a_missing_desktop_member_is_one_violation_at_the_root_manifest() {
    let ctx = Context {
        root: Path::new("/ws").to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    };
    assert_eq!(
        desktop_dir(&ctx),
        Err(Inspection::stopped(violation(
            "Cargo.toml",
            1,
            "the desktop crate (package `clauseo-desktop`) is not a workspace member",
            Fix::DesktopNotMember,
        )))
    );
}

#[test]
fn the_real_repo_passes_every_static_check() {
    let root = crate::workspace::workspace_root();
    let workspace = crate::workspace::load(&root).expect("cargo metadata runs");
    let ctx = Context {
        members: Ok(workspace),
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

/// The checks that read the workspace members, so stop when `cargo metadata` failed.
const READ_MEMBERS: [&str; 5] = [
    "capabilities",
    "plugins",
    "tauri-config",
    "dependencies",
    "core-io",
];

/// On the real repo with a failed `cargo metadata`, each check that reads members prints one
/// failure that quotes cargo's first `error:` line, points at the note above, and carries the
/// one fix line. Nothing about the empty workspace that stood in for the load. The other checks
/// do not read members and pass as on a good load.
#[test]
fn a_failed_metadata_load_stops_every_check_that_reads_members() {
    let first = "error: failed to load manifest for workspace member `/ws/crates/desktop`";
    let error = format!(
        "`cargo metadata` failed:\n{first}\n\nCaused by:\n  failed to parse manifest at \
         `/ws/crates/desktop/Cargo.toml`\n\nCaused by:\n  error: an indented line is not quoted"
    );
    let ctx = Context {
        root: crate::workspace::workspace_root(),
        members: Err(error),
        load_resolved_graph: false,
    };
    for name in READ_MEMBERS {
        assert!(
            ALL.iter().any(|check| check.name == name),
            "no check {name}"
        );
    }
    for check in ALL {
        let report = check.report(&ctx);
        if READ_MEMBERS.contains(&check.name) {
            let expected = vec![format!(
                "could not read the workspace members: `cargo metadata` failed with {first}. \
                 The full error is in the note above\n    fix: fix the error quoted above so that \
                 `cargo metadata` succeeds; this check reads the workspace members from its output"
            )];
            assert_eq!((check.name, report.printed()), (check.name, expected));
            assert!(report.inspection.stopped, "{} did not stop", check.name);
            assert!(report.failed(), "{} passed", check.name);
            assert_eq!(
                report.counted,
                format!("{} inspected", check.counts.count(0))
            );
        } else {
            assert!(!report.failed(), "{}: {:?}", check.name, report.printed());
        }
    }
}

/// An error with no line that starts with `error:`, such as cargo that could not start, is
/// quoted whole.
#[test]
fn a_load_error_with_no_error_line_is_quoted_whole() {
    let error = "could not run `cargo metadata`: No such file or directory";
    assert_eq!(metadata_quote(error), error);
}

#[test]
fn every_check_inspects_nothing_in_an_empty_repo() {
    let dir = Scratch::new("checks-empty-repo");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
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
        let fix = report
            .nothing_inspected
            .map(|failure| failure.fix.to_string());
        if STOPS_IN_AN_EMPTY_REPO.contains(&check.name) {
            assert_eq!((check.name, fix.as_deref()), (check.name, None));
            assert!(report.inspection.stopped, "{} did not stop", check.name);
        } else {
            let normal = format!(
                "{} looks for {} {}; restore them, or, if they moved, update the consts {} builds \
                 that place from",
                check.name,
                check.counts.many,
                (check.place)(),
                check.source()
            );
            assert_eq!((check.name, fix), (check.name, Some(normal)));
        }
    }
}

/// The checks that stop before they look in an empty folder with no workspace members: the
/// desktop checks find no desktop crate, and docs, file-size and expect-allowlist cannot list
/// files without git. The others look and find nothing: budgets reads no budgets.toml,
/// buildtime-edits no ui/app/src, dependencies and core-io no Crates table.
const STOPS_IN_AN_EMPTY_REPO: [&str; 6] = [
    "capabilities",
    "plugins",
    "tauri-config",
    "docs",
    "file-size",
    "expect-allowlist",
];

/// A check that stopped on a violation prints that violation's fix and no second fix line. A
/// check that looked, or that stopped with nothing printed, gets the zero-inspected failure.
#[test]
fn a_check_that_stopped_prints_one_fix_line() {
    let gone = || violation("a.json", 1, "gone", crate::fix::Fix::Fake("restore it"));
    let check = fake(|_| Inspection::default());
    let looked = check.judge(Inspection::new(0, vec![gone()]));
    let normal = "tauri-config looks for capability files in the desktop crate's capabilities/; \
                  restore them, or, if they moved, update the consts \
                  xtask/src/checks/tauri_config.rs builds that place from";
    assert_eq!(
        looked
            .nothing_inspected
            .map(|failure| failure.fix.to_string()),
        Some(normal.to_owned())
    );
    let stopped = check.judge(Inspection::stopped(gone()));
    assert_eq!(stopped.printed(), ["a.json:1: gone\n    fix: restore it"]);
    assert!(stopped.failed());
    let silent = check.judge(Inspection::stopped_on(Vec::new()));
    assert_eq!(
        silent
            .nothing_inspected
            .map(|failure| failure.fix.to_string()),
        Some(normal.to_owned()),
        "a stop with nothing printed above must still fail with a fix"
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

/// Every entry in `ALL` is a whole check: a unique name, the file it lives in and a place. Its
/// planted cases are held to the registry rule by `planted::tests`. A check left out of `ALL` is
/// caught by the compiler instead: nothing outside tests reads its `CHECK`, so clippy's dead-code
/// lint fails.
#[test]
fn every_check_in_all_has_a_unique_name_its_file_and_a_place() {
    let root = crate::workspace::workspace_root();
    let mut names = std::collections::BTreeSet::new();
    for check in ALL {
        assert!(names.insert(check.name), "{} is in ALL twice", check.name);
        let source = check.source();
        assert!(
            source.starts_with("xtask/src/checks/") && root.join(&source).is_file(),
            "{} names {source}, not its file under xtask/src/checks/",
            check.name
        );
        assert!(!(check.place)().is_empty(), "{} has no place", check.name);
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

/// `file!()` writes `\` on Windows; the fix names the file with `/` on every OS.
#[test]
fn a_source_written_with_backslashes_is_named_with_slashes() {
    let check = StaticCheck {
        source: "xtask\\src\\checks\\tauri_config.rs",
        ..fake(|_| Inspection::default())
    };
    assert_eq!(check.source(), "xtask/src/checks/tauri_config.rs");
    let fix = check.nothing_inspected().fix.to_string();
    assert!(
        fix.contains("the consts xtask/src/checks/tauri_config.rs builds"),
        "{fix}"
    );
}

#[test]
fn zero_inspected_fails_without_a_location_and_names_where_the_check_looked() {
    let check = fake(|_| Inspection::default());
    let report = check.report(&desktop_context(Path::new("/ws")));
    assert!(report.failed());
    assert_eq!(report.counted, "0 capability files inspected");
    assert_eq!(
        report.nothing_inspected.map(|failure| failure.to_string()),
        Some("0 capability files inspected in the desktop crate's capabilities/\n    fix: tauri-config looks for capability files in the desktop crate's capabilities/; restore them, or, if they moved, update the consts xtask/src/checks/tauri_config.rs builds that place from".to_owned())
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

    let check = fake(|_| {
        Inspection::new(
            2,
            vec![violation(
                "a.json",
                3,
                "bad",
                crate::fix::Fix::Fake("fix it"),
            )],
        )
    });
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
            fix: crate::fix::Fix::Fake("install git"),
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
        ]
    );
}

#[test]
fn a_git_past_its_limit_says_so_and_tells_to_run_it_by_hand() {
    let error = GitError::TimedOut(
        "`git ls-files -z` did not finish within 1 min, so it was stopped".to_owned(),
    );
    let report = fake(|_| Inspection::default()).judge(unlisted("file-size", &error));
    assert_eq!(
        report.printed(),
        [
            "could not list the repo's files: `git ls-files -z` did not finish within 1 min, so \
             it was stopped\n    \
             fix: run that git command by hand to see why it does not finish: file-size lists the \
             files it reads with `git ls-files`",
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
