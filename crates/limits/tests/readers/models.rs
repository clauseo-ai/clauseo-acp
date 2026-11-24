//! Claude's model probe against `fake_agent`: `initialize`, then `session/options` with the
//! data folder as `cwd` and no MCP servers, mapped to the models and their efforts; a refused
//! answer gives none. Codex's `model/list` is played over in-memory pipes in
//! `clauseo-codex_appserver`'s `tests/client/models.rs`.

use std::ffi::OsString;

use clauseo_limits::ReadLimits;
use clauseo_protocol::AgentKind;
use futures::FutureExt as _;
use futures::executor::block_on;
use futures::future::join;

use crate::harness::{Play, Setup, SharedTimer, TestLauncher};

fn probe_once(setup: &Setup, scenario: &'static str) -> Option<clauseo_protocol::AgentModels> {
    let launcher = TestLauncher::new(Play::Scenario(scenario));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let models = block_on(reader.probe_models(AgentKind::Claude));
    let spec = launcher.specs().pop().expect("one launch");
    assert_eq!(
        spec.cwd(),
        setup.run_dir(),
        "it runs in the readers' run folder"
    );
    let args: Vec<OsString> = vec![setup.script().into_os_string()];
    assert_eq!(
        spec.args(),
        args.as_slice(),
        "the adapter itself, no helper"
    );
    models
}

#[test]
fn session_options_give_every_model_its_names_and_efforts() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let models = probe_once(&setup, "claude-session-options").expect("the probe answered");
    assert_eq!(models.current_model, "opus");
    assert_eq!(models.current_effort.as_deref(), Some("medium"));
    let names: Vec<&str> = models
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect();
    assert_eq!(names, ["Opus 5.5", "Fable 5.2", "Haiku 5.5"]);
    let opus = models.model("opus").unwrap();
    let efforts: Vec<&str> = opus
        .efforts
        .iter()
        .map(|effort| effort.name.as_str())
        .collect();
    assert_eq!(efforts, ["Low", "Medium", "High", "Xhigh", "Max"]);
    assert_eq!(models.model("fable").unwrap().efforts.len(), 3);
    assert_eq!(models.model("haiku").unwrap().efforts, []);
}

#[test]
fn a_refused_session_options_gives_none() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    assert_eq!(probe_once(&setup, "claude-session-options-refused"), None);
}

#[test]
fn an_agent_with_no_entry_is_not_probed() {
    let setup = Setup::new();
    let launcher = TestLauncher::new(Play::Scenario("claude-session-options"));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    assert_eq!(block_on(reader.probe_models(AgentKind::Claude)), None);
    assert_eq!(launcher.launches(), 0);
}

#[test]
fn a_hold_ends_a_running_probe_and_its_process() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let launcher = TestLauncher::new(Play::Hold);
    let timer = SharedTimer::default();
    let reader = setup.readers(&launcher, &timer);
    let (models, guard) = block_on(join(reader.probe_models(AgentKind::Claude), async {
        // The probe's process runs, waiting on its limit.
        timer.0.next_armed().await;
        reader.hold(AgentKind::Claude).await
    }));
    assert_eq!(models, None);
    assert_eq!(launcher.launches(), 1);
    launcher.wait_stdout_closed();
    drop(guard);
}

#[test]
fn a_probe_asked_for_under_a_hold_launches_nothing() {
    let setup = Setup::new();
    setup.settings(&setup.script());
    let launcher = TestLauncher::new(Play::Scenario("claude-session-options"));
    let reader = setup.readers(&launcher, &SharedTimer::default());
    let guard = reader
        .hold(AgentKind::Claude)
        .now_or_never()
        .expect("nothing runs");
    assert_eq!(block_on(reader.probe_models(AgentKind::Claude)), None);
    assert_eq!(launcher.launches(), 0);
    drop(guard);
}
