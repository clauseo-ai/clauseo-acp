//! The runner's stderr lines: one `kind=` line a refusal, stable `snake_case` names, sentences with
//! no path or value, and no detail outside a `vm-harness-detail` build, whatever the environment.

use super::*;
use crate::windows::runner::RunExit;

/// A detail that names a path, as an error's text can.
const PATH: &str = r"D:\Clients\Acme v Beta\matter\.git";

/// The lines `write_line` writes for `kind` with a detail naming [`PATH`].
fn written(tag: &str, kind: RunnerKind) -> Vec<String> {
    let mut out = Vec::new();
    write_line(&mut out, tag, kind, Detail::new(|| PATH.to_owned())).expect("write");
    String::from_utf8(out)
        .expect("utf-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// What follows the line's `: `: its sentence.
fn after_colon(line: &str) -> &str {
    line.split_once(": ").map_or("", |(_, sentence)| sentence)
}

fn assert_path_free(text: &str) {
    for bad in ['\\', '/'] {
        assert!(!text.contains(bad), "{bad} in {text:?}");
    }
    assert!(!text.contains(":\\"), "a drive in {text:?}");
    assert!(
        !text.chars().any(|c| c.is_ascii_digit()),
        "a value in {text:?}"
    );
}

#[test]
fn every_refusing_exit_prints_exactly_one_kind_line() {
    for exit in RunExit::ALL {
        let Some(kind) = RunnerKind::for_exit(exit) else {
            assert_eq!(exit, RunExit::PoolStop, "only PoolStop has no kind");
            continue;
        };
        assert_eq!(kind.exit(), Some(exit), "{exit:?}");
        assert_eq!(RunnerKind::for_code(exit.code()), kind, "{exit:?}");
        let lines = written(REFUSAL, kind);
        let kinds: Vec<&String> = lines.iter().filter(|l| l.contains("kind=")).collect();
        assert_eq!(kinds.len(), 1, "{exit:?}: {lines:?}");
        assert_eq!(
            *kinds[0],
            format!(
                "ERR clauseo-sandbox-run kind={}: {}",
                kind.name(),
                kind.sentence()
            )
        );
        assert_path_free(after_colon(kinds[0]));
        #[cfg(not(feature = "vm-harness-detail"))]
        assert_eq!(lines.len(), 1, "{exit:?}: {lines:?}");
    }
}

#[test]
fn every_refusal_kind_names_an_exit_and_every_notice_none() {
    for kind in RunnerKind::all() {
        match kind.class() {
            KindClass::Notice => assert_eq!(kind.exit(), None, "{kind:?}"),
            _ if kind == RunnerKind::NotWindows => assert_eq!(kind.exit(), None),
            KindClass::Setup
            | KindClass::Admin
            | KindClass::Transient
            | KindClass::Permanent
            | KindClass::Internal => {
                let exit = kind.exit().expect("a refusal's exit");
                // A finer kind shares its exit with the exit's own kind.
                let own = RunnerKind::for_exit(exit).expect("the exit's own kind");
                assert_eq!(own.exit(), Some(exit), "{kind:?}");
            }
        }
        let lines = written(if kind.is_notice() { NOTICE } else { REFUSAL }, kind);
        assert_eq!(
            lines.iter().filter(|l| l.contains("kind=")).count(),
            1,
            "{lines:?}"
        );
    }
    // A code that isn't the runner's is said as its catch-all.
    assert_eq!(RunnerKind::for_code(1), RunnerKind::Internal);
    assert_eq!(
        RunnerKind::for_code(RunExit::PoolStop.code()),
        RunnerKind::Internal
    );
}

#[test]
fn names_are_unique_snake_case_and_cross_ctl_as_they_are() {
    let mut seen = std::collections::BTreeSet::new();
    for kind in RunnerKind::all() {
        let name = kind.name();
        assert!(seen.insert(name), "{name} twice");
        assert!(
            name.split('_').all(|part| !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())),
            "{name} isn't snake_case"
        );
        assert!(name.starts_with(|c: char| c.is_ascii_lowercase()), "{name}");
        // The inner sends kinds over `ctl` by serde, under the same name.
        let json = serde_json::to_string(&kind).expect("serialize");
        assert_eq!(json, format!("\"{name}\""));
        assert_eq!(serde_json::from_str::<RunnerKind>(&json).ok(), Some(kind));
    }
    assert_eq!(seen.len(), 54);
}

#[test]
fn the_table_is_in_the_enum_order() {
    for (at, kind) in RunnerKind::all().enumerate() {
        assert_eq!(kind as usize, at, "{kind:?}");
    }
}

#[test]
fn sentences_hold_no_path_and_no_value() {
    for kind in RunnerKind::all() {
        let sentence = kind.sentence();
        assert!(!sentence.trim().is_empty(), "{kind:?}");
        assert!(!sentence.contains('{'), "{kind:?}");
        assert!(!sentence.contains('\n'), "{kind:?}");
        assert_path_free(sentence);
    }
}

/// In the default build the detail is never written: no `DETAIL` line, no path.
#[cfg(not(feature = "vm-harness-detail"))]
#[test]
fn a_refusal_whose_error_holds_a_path_prints_no_path() {
    for kind in RunnerKind::all() {
        let tag = if kind.is_notice() { NOTICE } else { REFUSAL };
        let lines = written(tag, kind);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(!lines[0].contains("DETAIL"), "{lines:?}");
        assert!(!lines[0].contains("Acme"), "{lines:?}");
        assert_path_free(after_colon(&lines[0]));
    }
    // Nor does the inner send it over `ctl`.
    assert_eq!(Detail::new(|| PATH.to_owned()).to_send(), None);
}

/// A harness build writes the detail after the kind line, each line prefixed.
#[cfg(feature = "vm-harness-detail")]
#[test]
fn a_harness_build_writes_the_detail_after_the_kind_line() {
    let lines = written(REFUSAL, RunnerKind::MatterUnwatchable);
    assert_eq!(
        lines,
        [
            format!(
                "ERR clauseo-sandbox-run kind=matter_unwatchable: {}",
                RunnerKind::MatterUnwatchable.sentence()
            ),
            format!("DETAIL {PATH}"),
        ]
    );
    assert_eq!(
        Detail::new(|| PATH.to_owned()).to_send().as_deref(),
        Some(PATH)
    );
}

/// The real `refuse` and `notice`, on this process's stderr: the child half of
/// [`no_environment_variable_turns_the_detail_on`], harmless on its own.
#[test]
fn child_writes_a_refusal_and_a_notice() {
    refuse(
        RunnerKind::MatterUnwatchable,
        Detail::new(|| PATH.to_owned()),
    );
    notice(RunnerKind::LaunchFileKept, Detail::new(|| PATH.to_owned()));
}

/// The variables a reader might try, each set for a child run of this test binary that refuses
/// with a path in its detail: in the default build, its stderr has the kind lines and no
/// `DETAIL` line, whatever the environment.
#[cfg(not(feature = "vm-harness-detail"))]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs its own binary once, to read the real stderr under set variables"
)]
fn no_environment_variable_turns_the_detail_on() {
    let exe = std::env::current_exe().expect("this test binary");
    let mut child = std::process::Command::new(exe);
    child.args([
        "--exact",
        "windows::runner::stderr::tests::child_writes_a_refusal_and_a_notice",
        "--nocapture",
        "--test-threads=1",
    ]);
    for name in [
        "VM_HARNESS_DETAIL",
        "CLAUSEO_VM_HARNESS_DETAIL",
        "CLAUSEO_SANDBOX_DETAIL",
        "CLAUSEO_RUNNER_DETAIL",
        "CLAUSEO_DEBUG",
        "CLAUSEO_LOG",
        "RUST_LOG",
        "RUST_BACKTRACE",
        "DETAIL",
        "DEBUG",
    ] {
        child.env(name, "1");
    }
    let out = child.output().expect("the child run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("ERR clauseo-sandbox-run kind=matter_unwatchable: "),
        "{stderr}"
    );
    assert!(
        stderr.contains("NOTE clauseo-sandbox-run kind=launch_file_kept: "),
        "{stderr}"
    );
    assert!(!stderr.contains("DETAIL"), "{stderr}");
    assert!(!stderr.contains("Acme"), "{stderr}");
}

/// Nothing in the module reads the environment: the feature alone turns the detail on.
#[test]
fn the_module_reads_no_environment_variable() {
    for (file, text) in [
        ("stderr.rs", include_str!("../stderr.rs")),
        ("kinds.rs", include_str!("kinds.rs")),
        ("kinds/table.rs", include_str!("kinds/table.rs")),
    ] {
        for read in ["env::var", "var_os", "env!(", "option_env!", "vars()"] {
            assert!(!text.contains(read), "{file} reads {read}");
        }
    }
}

/// 0049's table lists every kind once, with its exit code, class and sentence.
#[test]
fn adr_0049_lists_every_kind() {
    let adr = include_str!("../../../../../../docs/decisions/0049-windows-sandbox-tier-b.md");
    for kind in RunnerKind::all() {
        let start = format!("| `{}` |", kind.name());
        let rows: Vec<&str> = adr.lines().filter(|l| l.starts_with(&start)).collect();
        assert_eq!(rows.len(), 1, "{}: {rows:?}", kind.name());
        let row = rows[0];
        let code = kind.exit().map_or_else(
            || "none".to_owned(),
            |exit| format!("0x{:08X}", exit.code()),
        );
        assert!(row.contains(&code), "{row} lacks {code}");
        assert!(
            row.contains(&format!("| {} |", kind.class().name())),
            "{row}"
        );
        assert!(row.contains(kind.sentence()), "{row}");
    }
}
