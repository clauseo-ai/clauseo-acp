//! The Python runtime's run check in an install job: a start the OS refuses is `RunBlocked`,
//! naming the policy its error code names; a check that fails or hangs is `RunFailed`. Either
//! way the tree goes, the archive stays, and the agent stays `Ready`.

use clauseo_agents::PythonBlocker;
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};

use super::*;

/// What a refused start returns.
type Refusal = fn() -> io::Error;

/// The passthrough launcher, refusing every `python3` with the error `refusal` makes.
struct RefusePython(PassthroughLauncher, Refusal);

impl Launcher for RefusePython {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if spec.program().ends_with("python3") {
            return Err(clauseo_process::Error::Spawn {
                program: spec.program().to_owned(),
                source: (self.1)(),
            });
        }
        self.0.launch(spec)
    }
}

/// A started with a launcher refusing `python3` with `refusal`: the agents and Python's end.
fn refused_with(fx: &Fixture, refusal: Refusal) -> (Agents, Events, PythonState) {
    let passthrough = PassthroughLauncher::new(SpawnOptions::new());
    let launcher: Arc<dyn Launcher> = Arc::new(RefusePython(passthrough, refusal));
    let (agents, events) = Agents::start(fx.config.clone(), launcher, fx.timer.clone());
    let events = Events::new(events);
    events.first_states();
    let last = install_a(&agents, &events);
    (agents, events, last)
}

fn blocked_by(state: &PythonState) -> Option<PythonBlocker> {
    match state {
        PythonState::Failed(failure) => failure.blocked_by,
        other => panic!("not failed: {other:?}"),
    }
}

#[test]
fn a_refused_python_is_run_blocked_and_the_agent_stays_ready() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-refused", &bytes);
    let (agents, _events, last) =
        refused_with(&fx, || io::Error::from(io::ErrorKind::PermissionDenied));
    assert_eq!(failure_kind(&last), PythonFailureKind::RunBlocked);
    assert_eq!(blocked_by(&last), Some(PythonBlocker::Unknown));
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
    assert!(!runtime_dir(&fx, 1).exists(), "the refused tree is removed");
    assert!(download(&fx).exists(), "the archive stays for a retry");
    assert_eq!(fx.timer.install_waits(), Vec::<Duration>::new(), "no retry");
}

#[test]
fn a_run_blocked_python_names_the_policy_its_error_code_names() {
    let cases: [(Refusal, PythonBlocker, &str); 3] = [
        (
            || io::Error::from_raw_os_error(4551),
            PythonBlocker::AppControl,
            "app_control",
        ),
        (
            || io::Error::from_raw_os_error(1260),
            PythonBlocker::AppLocker,
            "applocker",
        ),
        (
            || io::Error::from_raw_os_error(0xC000_0428_u32.cast_signed()),
            PythonBlocker::Unknown,
            "unknown",
        ),
    ];
    for (refusal, want, word) in cases {
        let bytes = fake_python("echo 3.13.18");
        let fx = with_python(&format!("py-refused-{word}"), &bytes);
        let (_agents, _events, last) = refused_with(&fx, refusal);
        assert_eq!(failure_kind(&last), PythonFailureKind::RunBlocked);
        assert_eq!(blocked_by(&last), Some(want));
        let PythonState::Failed(failure) = &last else {
            unreachable!()
        };
        let log = failure.log.to_string();
        assert!(log.contains(&format!("blocked by: {word}")), "{log}");
    }
}

#[test]
fn a_python_whose_check_exits_non_zero_is_run_failed() {
    let bytes = fake_python("exit 2");
    let fx = with_python("py-exit", &bytes);
    let (agents, events) = fx.start();
    events.first_states();
    let last = install_a(&agents, &events);
    assert_eq!(failure_kind(&last), PythonFailureKind::RunFailed);
    assert_eq!(blocked_by(&last), None);
    let PythonState::Failed(failure) = &last else {
        unreachable!()
    };
    let log = failure.log.to_string();
    assert!(log.contains("exited with code 2"), "{log}");
    // The failed try's read-only tree is gone, temp folder included.
    let runtime = fx.data.path().join("agents/runtime");
    let left: Vec<_> = std::fs::read_dir(&runtime).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_python_that_does_not_answer_in_20_seconds_is_run_failed() {
    let bytes = fake_python("sleep 600");
    let fx = with_python("py-hang", &bytes);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Ready { .. }));
    events.until_python(|s| *s == PythonState::Installing);
    fx.timer.fire_next(secs(20));
    let (_, last) = events.until_python_end();
    assert_eq!(failure_kind(&last), PythonFailureKind::RunFailed);
    assert!(!runtime_dir(&fx, 1).exists());
}

#[test]
fn an_archive_with_no_interpreter_leaves_no_read_only_temp_folder() {
    let bytes = runtime_tar_gz(
        "python",
        &[
            ("lib/", 0o755, b""),
            ("lib/deep/", 0o755, b""),
            ("lib/deep/x.py", 0o644, b""),
        ],
    )
    .unwrap();
    let fx = with_python("py-no-interpreter", &bytes);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until_end(&agent_a());
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    let (_, last) = events.until_python_end();
    assert_eq!(failure_kind(&last), PythonFailureKind::Failed);
    let runtime = fx.data.path().join("agents/runtime");
    let left: Vec<_> = std::fs::read_dir(&runtime).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}
