//! The lines and results `explain.rs` gives, and the removal's exit once the admin copy's
//! removal is known.

use super::*;

#[test]
fn codes_read_as_windows_names_them() {
    assert_eq!(Code::Win32(5).to_string(), "Win32 error 5");
    assert_eq!(Code::Hresult(0x8007_0005).to_string(), "HRESULT 0x80070005");
    assert_eq!(
        (Code::Win32(5).kind(), Code::Hresult(1).kind()),
        ("win32", "hresult")
    );
    let os = Cause::from(MachineError::Os {
        call: "RegOpenKeyExW",
        code: 2,
    });
    assert_eq!(os.code, Some(Code::Win32(2)));
    assert_eq!(Cause::from(MachineError::Other("x".to_owned())).code, None);
    let io = Cause::from(std::io::Error::from_raw_os_error(32));
    assert_eq!(io.code, Some(Code::Win32(32)));
    assert_eq!(Cause::from(std::io::Error::other("x")).code, None);
}

#[test]
fn a_failure_without_a_code_says_so_and_writes_no_code() {
    let failure = AdoptFailure {
        step: AdoptStep::DllSearch,
        cause: Cause::from("refused"),
    };
    assert_eq!(
        failure.line(),
        "--adopt failed (exit 3) at step dll_search: no Windows error code"
    );
    let result = failure.result();
    assert_eq!((result.code, result.kind), (None, None));
    assert_eq!(result.cause.as_deref(), Some("refused"));
}

#[test]
fn a_finished_removal_is_removed_and_an_admin_copy_left_is_reported() {
    let mut lines = Vec::new();
    assert_eq!(
        removal_exit(Ok(()), &mut |line| lines.push(line.to_owned())),
        SetupExit::Removed
    );
    assert_eq!(lines, Vec::<String>::new());
    let left = CopyLeft {
        step: "admin_folder",
        cause: Cause::with(
            r"C:\Program Files\Clauseo\Sandbox\staging: denied",
            Code::Win32(5),
        ),
    };
    let exit = removal_exit(Err(left), &mut |line| lines.push(line.to_owned()));
    assert_eq!(exit, SetupExit::AdminCopyLeft);
    assert_eq!(exit.as_code(), 10);
    assert_eq!(
        lines,
        ["--remove --uninstall left the admin copy (exit 10) at step admin_folder: Win32 error 5"]
    );
}

#[test]
fn every_steps_name_is_distinct() {
    let steps = [
        AdoptStep::DllSearch,
        AdoptStep::Token,
        AdoptStep::ProgramFiles,
        AdoptStep::RunningImage,
        AdoptStep::Lawyer,
        AdoptStep::Lock,
        AdoptStep::Record,
        AdoptStep::Profile,
        AdoptStep::LockDown,
        AdoptStep::Hash,
        AdoptStep::Task(SystemTask::Refresh),
        AdoptStep::Task(SystemTask::Stop),
        AdoptStep::Values,
        AdoptStep::SetUpUser,
    ];
    let mut names: Vec<&str> = steps.iter().map(|step| step.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), steps.len());
}
