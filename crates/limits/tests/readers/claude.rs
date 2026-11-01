//! Claude reads against `fake_agent`: the outcome of each scenario, the spec the reader built,
//! the helper file it wrote, and the timeout's kill.

use std::time::{Duration, SystemTime};

use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{AgentKind, Billing, LimitSource, PlanLabel};
use futures::executor::block_on;
use futures::future::join;

use crate::harness::{Play, Setup, SharedTimer, TestLauncher};
use crate::write;

/// The helper the reader must write, byte for byte.
const EMBEDDED: &str = include_str!("../../helper/claude-usage.mjs");

/// Reads Claude's limits once with `fake_agent` playing `scenario`, under a timer never fired.
fn read_once(setup: &Setup, scenario: &'static str) -> (ReadOutcome, std::sync::Arc<TestLauncher>) {
    let launcher = TestLauncher::new(Play::Scenario(scenario));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Claude));
    (outcome, launcher)
}

fn ready_setup() -> Setup {
    let setup = Setup::new();
    setup.settings(&setup.script());
    setup
}

#[test]
fn one_line_is_read_and_mapped_after_the_helper_exits() {
    let setup = ready_setup();
    let (outcome, launcher) = read_once(&setup, "claude-ok");
    #[cfg(unix)]
    launcher.wait_stdout_closed();
    let ReadOutcome::Usage { limits, reached } = outcome else {
        panic!("not usage: {outcome:?}");
    };
    assert_eq!(reached, None);
    assert_eq!(limits.account.0, "max.user@example.com");
    assert_eq!(limits.plan, Some(PlanLabel("Claude Max".to_owned())));
    assert_eq!(limits.billing, Billing::Plan);
    assert!(matches!(limits.source, LimitSource::Fresh { .. }));
    let ids: Vec<&str> = limits
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["five_hour", "seven_day", "seven_day_overage_included"]
    );
}

#[test]
fn the_spec_runs_settings_command_on_the_helper_in_the_run_folder() {
    let setup = ready_setup();
    let (_, launcher) = read_once(&setup, "claude-ok");
    let specs = launcher.specs();
    let [spec] = specs.as_slice() else {
        panic!("expected one launch, got {}", specs.len());
    };
    assert_eq!(spec.program(), std::path::Path::new(&Setup::command()));
    assert_eq!(spec.args(), [setup.helper().into_os_string()]);
    assert_eq!(spec.cwd(), setup.run_dir());
    assert!(setup.run_dir().is_dir());
    assert_eq!(Setup::names(&setup.run_dir()), Vec::<String>::new());
    assert_eq!(spec.env(), &Setup::expected_env());
}

#[test]
fn the_helper_is_written_once_and_left_alone_while_it_matches() {
    let setup = ready_setup();
    let (first, _) = read_once(&setup, "claude-ok");
    assert!(matches!(first, ReadOutcome::Usage { .. }), "{first:?}");
    assert_eq!(std::fs::read_to_string(setup.helper()).unwrap(), EMBEDDED);
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    write::set_modified(&setup.helper(), old).unwrap();
    let (second, _) = read_once(&setup, "claude-ok");
    assert!(matches!(second, ReadOutcome::Usage { .. }), "{second:?}");
    assert_eq!(
        std::fs::metadata(setup.helper())
            .unwrap()
            .modified()
            .unwrap(),
        old
    );
    assert_eq!(
        Setup::names(&setup.install()),
        ["clauseo-claude-usage.mjs", "node_modules"],
        "no temp file is left in the install folder"
    );
}

#[test]
fn a_different_helper_file_is_replaced() {
    let setup = ready_setup();
    Setup::file(&setup.helper(), "// an older helper\n");
    let (outcome, _) = read_once(&setup, "claude-ok");
    assert!(matches!(outcome, ReadOutcome::Usage { .. }), "{outcome:?}");
    assert_eq!(std::fs::read_to_string(setup.helper()).unwrap(), EMBEDDED);
}

#[test]
fn a_crash_reports_its_code_and_the_redacted_tail() {
    let setup = ready_setup();
    let (outcome, launcher) = read_once(&setup, "claude-crash");
    #[cfg(unix)]
    launcher.wait_stdout_closed();
    let ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }) = outcome else {
        panic!("not an exit: {outcome:?}");
    };
    assert_eq!(code, Some(1));
    assert!(
        !stderr_tail.contains(crate::harness::FAKE_KEY),
        "{stderr_tail}"
    );
    assert!(
        stderr_tail.contains("the key [redacted] was refused"),
        "{stderr_tail}"
    );
    assert!(
        stderr_tail.contains("claude-usage: starting the SDK"),
        "{stderr_tail}"
    );
    assert!(
        stderr_tail.contains("claude-usage: giving up"),
        "{stderr_tail}"
    );
}

#[test]
fn no_line_is_malformed() {
    let setup = ready_setup();
    let (outcome, _) = read_once(&setup, "claude-silent");
    assert_eq!(
        outcome,
        ReadOutcome::Failed(ReadFailure::Malformed("no line".to_owned()))
    );
}

#[test]
fn a_line_that_is_not_json_is_malformed() {
    let setup = ready_setup();
    let (outcome, _) = read_once(&setup, "claude-garbage");
    assert_eq!(
        outcome,
        ReadOutcome::Failed(ReadFailure::Malformed("the line is not JSON".to_owned()))
    );
}

#[test]
fn stderr_past_the_pipe_buffer_is_drained_and_the_line_still_read() {
    let setup = ready_setup();
    let (outcome, _) = read_once(&setup, "claude-stderr-heavy");
    assert!(matches!(outcome, ReadOutcome::Usage { .. }), "{outcome:?}");
}

#[test]
fn stdout_past_the_pipe_buffer_is_drained_to_its_end() {
    let setup = ready_setup();
    let (outcome, launcher) = read_once(&setup, "claude-stdout-heavy");
    assert!(matches!(outcome, ReadOutcome::Usage { .. }), "{outcome:?}");
    #[cfg(unix)]
    launcher.wait_stdout_closed();
}

#[test]
fn the_deadline_is_25_seconds_and_kills_the_tree() {
    let setup = ready_setup();
    let launcher = TestLauncher::new(Play::Hold);
    let timer = SharedTimer::default();
    let reader = setup.readers(&launcher, &timer);
    let (outcome, ()) = block_on(join(reader.read(AgentKind::Claude), async {
        assert_eq!(timer.0.next_armed().await, Duration::from_secs(25));
        assert!(timer.0.fire());
    }));
    assert_eq!(outcome, ReadOutcome::Failed(ReadFailure::TimedOut));
    assert_eq!(launcher.specs().len(), 1);
    #[cfg(unix)]
    launcher.wait_stdout_closed();
}
