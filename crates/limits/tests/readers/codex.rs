//! Codex reads against `fake_agent`: exit, timeout and an empty answer, the spec the reader
//! built (with `CODEX_HOME` forced), and where `codex.js` is found.
//!
//! No test plays a full app-server exchange: `fake_agent` takes only client lines carrying
//! `"jsonrpc": "2.0"`, which `codex_appserver` never writes, and its `expect` cannot match the
//! `initialized` notification. `src/readers/codex/tests/talk.rs` plays the protocol over
//! in-memory pipes instead.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::AgentKind;
use futures::executor::block_on;
use futures::future::join;

use crate::harness::{CODEX_JS, FAKE_OPENAI_KEY, Play, Setup, SharedTimer, TestLauncher, under};

/// A Codex setup whose install holds `codex.js` in the install's `node_modules`.
fn codex_setup() -> (Setup, PathBuf) {
    let setup = Setup::new();
    Setup::file(&setup.codex_script(), "");
    let codex_js = under(&setup.install().join("node_modules"), CODEX_JS);
    Setup::file(&codex_js, "");
    setup.codex_settings(&setup.codex_script());
    crate::write::create_dir_all(&setup.codex_home()).unwrap();
    (setup, codex_js)
}

/// Reads Codex's limits once with `fake_agent` playing `play`, under a timer never fired.
fn read_once(setup: &Setup, play: Play) -> (ReadOutcome, std::sync::Arc<TestLauncher>) {
    let launcher = TestLauncher::new(play);
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Codex));
    (outcome, launcher)
}

#[test]
fn a_crash_reports_its_code_and_the_redacted_tail() {
    let (setup, _) = codex_setup();
    let (outcome, launcher) = read_once(&setup, Play::Scenario("codex-crash"));
    let ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }) = outcome else {
        panic!("not an exit: {outcome:?}");
    };
    assert_eq!(code, Some(1));
    assert!(!stderr_tail.contains(FAKE_OPENAI_KEY), "{stderr_tail}");
    assert!(
        stderr_tail.contains("the key [redacted] was refused"),
        "{stderr_tail}"
    );
    assert!(
        stderr_tail.contains("codex app-server: giving up"),
        "{stderr_tail}"
    );
    launcher.wait_stdout_closed();
}

#[test]
fn stderr_past_the_pipe_buffer_is_drained_and_its_tail_redacted() {
    let (setup, _) = codex_setup();
    let (outcome, _) = read_once(&setup, Play::Scenario("codex-stderr-heavy"));
    let ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }) = outcome else {
        panic!("not an exit: {outcome:?}");
    };
    assert_eq!(code, Some(1));
    assert!(stderr_tail.len() <= 4096, "{}", stderr_tail.len());
    assert!(!stderr_tail.contains(FAKE_OPENAI_KEY), "{stderr_tail}");
    assert!(
        stderr_tail.ends_with("codex app-server: the key [redacted] was refused\n"),
        "{stderr_tail}"
    );
    assert!(
        stderr_tail.contains("codex app-server: stderr filler"),
        "{stderr_tail}"
    );
}

#[test]
fn a_missing_codex_home_is_signed_out_without_a_launch() {
    let (setup, _) = codex_setup();
    crate::write::remove_dir_all(&setup.codex_home()).unwrap();
    let (outcome, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    assert_eq!(outcome, ReadOutcome::SignedOut);
    assert_eq!(launcher.launches(), 0);
    assert!(!setup.codex_home().exists(), "the home is never made");
}

#[cfg(unix)]
#[test]
fn a_linked_codex_home_is_refused_without_a_launch() {
    let (setup, _) = codex_setup();
    crate::write::remove_dir_all(&setup.codex_home()).unwrap();
    let real = setup.root.join("real-home");
    crate::write::create_dir_all(&real).unwrap();
    crate::write::symlink(&real, &setup.codex_home()).unwrap();
    let (outcome, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
        panic!("not a refusal: {outcome:?}");
    };
    assert!(message.contains("is a link"), "{message}");
    assert_eq!(launcher.launches(), 0);
}

#[test]
fn an_app_server_that_exits_at_once_is_malformed() {
    let (setup, _) = codex_setup();
    let (outcome, _) = read_once(&setup, Play::Scenario("codex-silent"));
    assert_eq!(
        outcome,
        ReadOutcome::Failed(ReadFailure::Malformed(
            "the app-server closed the connection".to_owned()
        ))
    );
}

#[test]
fn the_deadline_is_10_seconds_and_kills_the_tree() {
    let (setup, _) = codex_setup();
    let launcher = TestLauncher::new(Play::Hold);
    let timer = SharedTimer::default();
    let reader = setup.readers(&launcher, &timer);
    let (outcome, ()) = block_on(join(reader.read(AgentKind::Codex), async {
        assert_eq!(timer.0.next_armed().await, Duration::from_secs(10));
        assert!(timer.0.fire());
    }));
    assert_eq!(outcome, ReadOutcome::Failed(ReadFailure::TimedOut));
    assert_eq!(launcher.launches(), 1);
    launcher.wait_stdout_closed();
}

#[test]
fn the_spec_runs_codex_js_app_server_with_codex_home_forced() {
    let (setup, codex_js) = codex_setup();
    let (_, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    let specs = launcher.specs();
    let [spec] = specs.as_slice() else {
        panic!("expected one launch, got {}", specs.len());
    };
    assert_eq!(spec.program(), std::path::Path::new(&Setup::command()));
    let mut args = vec![codex_js.into_os_string()];
    for set in [
        "features.apps=false",
        "features.plugins=false",
        "features.skill_mcp_dependency_install=false",
        "cloud.skills.enabled=false",
        "skills.bundled.enabled=false",
    ] {
        args.extend([OsString::from("-c"), OsString::from(set)]);
    }
    args.push(OsString::from("app-server"));
    assert_eq!(
        spec.args(),
        args,
        "#203: the host's switches, before the subcommand"
    );
    assert_eq!(spec.cwd(), setup.run_dir());
    let homes: Vec<_> = spec
        .env()
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("codex_home"))
        .collect();
    assert_eq!(
        homes,
        [(
            &OsString::from("CODEX_HOME"),
            &setup.codex_home().into_os_string()
        )]
    );
    assert!(
        spec.env().get(std::ffi::OsStr::new("OPENAI_API_KEY"))
            == Some(&OsString::from(FAKE_OPENAI_KEY)),
        "OPENAI_API_KEY is not the settings value"
    );
    assert_eq!(
        Setup::names(&setup.codex_home()),
        Vec::<String>::new(),
        "the home stays empty"
    );
    assert_eq!(
        Setup::names(&setup.install()),
        ["node_modules"],
        "no helper is written"
    );
}

#[test]
fn codex_acps_own_codex_js_wins_over_the_installs() {
    let (setup, _) = codex_setup();
    let own = under(
        &under(
            &setup.install(),
            "node_modules/@agentclientprotocol/codex-acp/node_modules",
        ),
        CODEX_JS,
    );
    Setup::file(&own, "");
    let (_, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    let specs = launcher.specs();
    let args = specs.first().expect("one launch").args();
    assert_eq!(args.first(), Some(&own.into_os_string()));
}

#[test]
fn no_codex_js_is_refused_and_nothing_launched() {
    let setup = Setup::new();
    Setup::file(&setup.codex_script(), "");
    setup.codex_settings(&setup.codex_script());
    let (outcome, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
        panic!("not a refusal: {outcome:?}");
    };
    assert!(
        message.contains("no @openai/codex/bin/codex.js"),
        "{message}"
    );
    assert_eq!(launcher.launches(), 0);
    assert!(!setup.run_dir().exists());
}

#[cfg(unix)]
#[test]
fn a_linked_openai_codex_package_is_refused() {
    let setup = Setup::new();
    Setup::file(&setup.codex_script(), "");
    let real = under(&setup.root, "store/codex");
    Setup::file(&under(&real, "bin/codex.js"), "");
    let openai = under(&setup.install(), "node_modules/@openai");
    crate::write::create_dir_all(&openai).unwrap();
    crate::write::symlink(&real, &openai.join("codex")).unwrap();
    setup.codex_settings(&setup.codex_script());
    let (outcome, launcher) = read_once(&setup, Play::Scenario("codex-silent"));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(message)) = outcome else {
        panic!("not a refusal: {outcome:?}");
    };
    assert!(
        message.contains("is a link inside the install folder"),
        "{message}"
    );
    assert_eq!(launcher.launches(), 0);
}

#[test]
fn no_codex_entry_is_refused() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let (outcome, launcher) = read_once(&setup, Play::Hold);
    assert_eq!(
        outcome,
        ReadOutcome::Failed(ReadFailure::LaunchFailed(
            "no settings entry for codex-acp".to_owned()
        ))
    );
    assert_eq!(launcher.launches(), 0);
}
