//! Every `--adopt` exit 3 before step 8 reports its step and its error's code
//! (`Adopter::report`), and an unrecorded copy with no record is adopted.

use super::*;

/// Runs `--adopt` per user with `step` failing; its exit and what it reported.
fn failing_at(step: AdoptStep) -> (SetupExit, Vec<AdoptFailure>) {
    let mut fake = Fake::new();
    fake.fail = Some(step);
    let exit = adopt(&mut fake, Path::new(INSTALL), &user());
    (exit, fake.reports)
}

#[test]
fn every_early_failed_exit_reports_its_step_and_code() {
    for step in [
        AdoptStep::Token,
        AdoptStep::ProgramFiles,
        AdoptStep::RunningImage,
        AdoptStep::Lock,
        AdoptStep::Record,
        AdoptStep::Profile,
        AdoptStep::LockDown,
        AdoptStep::Hash,
        AdoptStep::Values,
    ] {
        let (exit, reports) = failing_at(step);
        assert_eq!(exit, SetupExit::Failed, "{step:?}");
        assert_eq!(reports.len(), 1, "{step:?}: {reports:?}");
        assert_eq!(reports[0].step, step);
        assert_eq!(reports[0].cause.code, Some(Code::Win32(5)), "{step:?}");
        let result = reports[0].result();
        assert_eq!(
            (
                result.exit,
                result.step.as_deref(),
                result.code,
                result.kind.as_deref()
            ),
            (3, Some(step.name()), Some(5), Some("win32")),
        );
        let line = reports[0].line();
        assert_eq!(
            line,
            format!(
                "--adopt failed (exit 3) at step {}: Win32 error 5",
                step.name()
            )
        );
    }
}

#[test]
fn the_lawyer_a_task_and_a_bad_hash_report_their_steps_too() {
    let mut fake = Fake::new();
    fake.outer_user = None;
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Failed
    );
    assert_eq!(fake.reports[0].step, AdoptStep::Lawyer);
    assert_eq!(fake.reports[0].cause.code, None);
    assert!(
        fake.reports[0]
            .line()
            .ends_with("lawyer: no Windows error code")
    );

    for task in [SystemTask::Refresh, SystemTask::Stop] {
        let mut fake = Fake::new();
        fake.failing_task = Some(task);
        assert_eq!(
            adopt(&mut fake, Path::new(INSTALL), &user()),
            SetupExit::Failed
        );
        assert_eq!(fake.reports.len(), 1);
        assert_eq!(fake.reports[0].step, AdoptStep::Task(task));
        let line = fake.reports[0].line();
        assert!(line.ends_with(": HRESULT 0x80041318"), "{line}");
    }
}

#[test]
fn the_event_log_line_names_no_path_or_sid() {
    let mut fake = Fake::new();
    fake.fail = Some(AdoptStep::LockDown);
    adopt(&mut fake, Path::new(INSTALL), &user());
    let line = fake.reports[0].line();
    for secret in [r"C:\", LAWYER, "S-1-5"] {
        assert!(!line.contains(secret), "{line}");
    }
}

#[test]
fn a_refusal_that_isnt_a_failure_and_a_success_report_nothing() {
    let mut fake = Fake::new();
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    assert!(fake.reports.is_empty(), "{:?}", fake.reports);
    let mut fake = Fake::new();
    fake.lock_busy = true;
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::LockHeld
    );
    assert!(fake.reports.is_empty(), "{:?}", fake.reports);
}

#[test]
fn an_unrecorded_copy_with_no_record_is_adopted() {
    // The copy is in place (the installer just wrote it) and the record holds none of its
    // values, as after a per-user uninstall: no owner, no copy owner, no version.
    let mut fake = Fake::new();
    assert_eq!(fake.found(Path::new(ADMIN_EXE)), Found::File);
    assert_eq!(fake.records().unwrap(), Records::default());
    assert_eq!(
        adopt(&mut fake, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    let version = admin_copy::APP_VERSION;
    assert!(
        fake.changes
            .contains(&format!("record {HASH} {INSTALL} {version} {LAWYER}")),
        "{:?}",
        fake.changes
    );
    assert_eq!(fake.changes.last(), Some(&format!("set-up {DATA}")));
    assert_eq!(fake.recorded.as_deref(), Some(HASH));
}
