//! The unpacked runtime's one run before its receipt: `python -I -c "import sys, lxml, numpy;
//! print(sys.version)"`, through the install's launcher, within [`SMOKE_LIMIT`]. A computer
//! whose policy (Smart App Control, `AppLocker`, WDAC) refuses to start the interpreter or load
//! its libraries says so here.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use futures::future::{Either, join3, select};

use crate::cancel::Cancel;
use crate::state::PythonBlocker;

/// How long the run may take.
pub(crate) const SMOKE_LIMIT: Duration = Duration::from_secs(20);

/// What the run is asked to do: start in isolated mode (no user site, no `PYTHON*` variables)
/// and load two of the runtime's native libraries.
const SMOKE_CODE: &str = "import sys, lxml, numpy; print(sys.version)";

/// Windows' errors for a program or library its policy refuses: `ERROR_ACCESS_DISABLED_BY_POLICY`
/// (1260), `ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION` (4551), and the NT statuses a process
/// refused at load exits with, `STATUS_SYSTEM_INTEGRITY_POLICY_VIOLATION` (`0xC0E90002`) and
/// `STATUS_INVALID_IMAGE_HASH` (`0xC0000428`).
const POLICY_ERRORS: [i32; 2] = [ACCESS_DISABLED_BY_POLICY, INTEGRITY_POLICY_VIOLATION];
const POLICY_STATUSES: [u32; 2] = [INTEGRITY_POLICY_STATUS, 0xC000_0428];

/// `ERROR_ACCESS_DISABLED_BY_POLICY`: `AppLocker` or Software Restriction Policies.
const ACCESS_DISABLED_BY_POLICY: i32 = 1260;
/// `ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION`: App Control for Business or Smart App Control.
const INTEGRITY_POLICY_VIOLATION: i32 = 4551;
/// `STATUS_SYSTEM_INTEGRITY_POLICY_VIOLATION`, the NT status 4551 comes from.
const INTEGRITY_POLICY_STATUS: u32 = 0xC0E9_0002;

/// Which policy a refusal's code names, from the code alone: 4551 (or its NT status) is App
/// Control, 1260 is `AppLocker`, anything else (access denied, an image hash) is unknown.
// TODO(#812): tell Smart App Control apart from other App Control policies with #812's
// Smart App Control registry read, once it is on main.
fn blocker(code: Option<i32>) -> PythonBlocker {
    match code {
        Some(INTEGRITY_POLICY_VIOLATION) => PythonBlocker::AppControl,
        Some(code) if code == INTEGRITY_POLICY_STATUS.cast_signed() => PythonBlocker::AppControl,
        Some(ACCESS_DISABLED_BY_POLICY) => PythonBlocker::AppLocker,
        _ => PythonBlocker::Unknown,
    }
}

/// Why the run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunFailure {
    /// The OS refused to start the interpreter or load its code: access denied or a policy,
    /// and which kind of policy its error code names.
    Blocked(String, PythonBlocker),
    /// It started but exited non-zero, printed nothing, or took longer than [`SMOKE_LIMIT`].
    Failed(String),
    /// The install was paused or quit meanwhile.
    Cancelled,
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunFailure::Blocked(detail, _) | RunFailure::Failed(detail) => f.write_str(detail),
            RunFailure::Cancelled => f.write_str("cancelled"),
        }
    }
}

/// Whether a launch error is the OS refusing the program: access denied, or one of
/// [`POLICY_ERRORS`].
fn refused(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
        || err.raw_os_error().is_some_and(|code| {
            POLICY_ERRORS.contains(&code)
                || POLICY_STATUSES
                    .iter()
                    .any(|&status| code == status.cast_signed())
        })
}

/// Runs `program` once as [`SMOKE_CODE`] says. Its stdout and stderr are read and dropped; the
/// exit is awaited within [`SMOKE_LIMIT`], past which the handle is dropped, which kills it.
pub(crate) async fn smoke<T: Timer>(
    program: &Path,
    launcher: &dyn Launcher,
    timer: &T,
    cancel: &Cancel,
) -> Result<(), RunFailure> {
    let cwd = program.parent().map(Path::to_path_buf).unwrap_or_default();
    let args = vec!["-I".into(), "-c".into(), SMOKE_CODE.into()];
    let spec = LaunchSpec::new(program.to_owned(), args, BTreeMap::new(), cwd)
        .map_err(|err| RunFailure::Failed(format!("Python could not be started: {err}")))?;
    let launched = launcher.launch(&spec).map_err(|err| match &err {
        clauseo_process::Error::Spawn { source, .. } if refused(source) => RunFailure::Blocked(
            format!("this computer refused to start Python: {err}"),
            blocker(source.raw_os_error()),
        ),
        _ => RunFailure::Failed(format!("Python could not be started: {err}")),
    })?;
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launched;
    drop(stdin);
    let ran = {
        let work = pin!(async {
            let read = async {
                let mut seen = 0_usize;
                let mut chunk = [0_u8; 4096];
                while let Ok(n @ 1..) = futures::AsyncReadExt::read(&mut stdout, &mut chunk).await {
                    seen = seen.saturating_add(n);
                }
                seen
            };
            let (seen, drained, report) = join3(read, stderr.drain(), handle.exited()).await;
            drop(drained);
            (seen, report.code())
        });
        let stop = pin!(async {
            select(pin!(timer.after(SMOKE_LIMIT)), pin!(cancel.cancelled())).await;
        });
        match select(work, stop).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    let Some((seen, code)) = ran else {
        drop(handle);
        if cancel.is_cancelled() {
            return Err(RunFailure::Cancelled);
        }
        return Err(RunFailure::Failed(format!(
            "Python did not answer within {} s",
            SMOKE_LIMIT.as_secs()
        )));
    };
    exit_result(seen, code)
}

/// What a run that ended means: `seen` bytes printed, exit `code`. A process refused at load
/// exits with one of [`POLICY_STATUSES`], which names its blocker as a refused start's error
/// does.
pub(super) fn exit_result(seen: usize, code: Option<i32>) -> Result<(), RunFailure> {
    match code {
        Some(0) if seen > 0 => Ok(()),
        Some(code) if POLICY_STATUSES.iter().any(|&s| code == s.cast_signed()) => {
            Err(RunFailure::Blocked(
                format!("this computer refused to load Python (code {code:#x})"),
                blocker(Some(code)),
            ))
        }
        Some(code) => Err(RunFailure::Failed(format!(
            "Python's check exited with code {code}"
        ))),
        None => Err(RunFailure::Failed(
            "Python's check ended without an exit code".to_owned(),
        )),
    }
}
