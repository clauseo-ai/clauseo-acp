//! The model probe on a fresh data folder (`actor/agent_options/probe.rs`): a reader whose read
//! finds the agent signed in probes its models, and `ListAgents` then names them, the agent's
//! own default included, before any conversation; a failed probe keeps what was kept; a pick of
//! the probed values before the first message is set at the launch, a prompt's own pick
//! (`SendPrompt::config`) just before that prompt, and each `TurnStarted`, the header and the
//! thread's row name the model and effort the turn runs with (`pickers-probed`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use clauseo_limits::{ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AgentId, AgentKind, AgentModels, CommandOutcome, DeliveryRequest, DomainEvent, HostCommand,
    ModelEntry, NamedChoice, StreamId, ThreadId, TurnStarted,
};

use crate::pickers::{CLAUDE, picks, turn};
use crate::pickers_seed::agent_summary;
use crate::support::{Dir, Running, Scenario, text};

/// A reader whose reads find the agent signed in (Claude's usage method missing) and whose
/// probe answers `models`, counting the probes.
struct ProbeReader {
    models: Option<AgentModels>,
    probes: Arc<AtomicUsize>,
}

impl ReadLimits for ProbeReader {
    fn read(&self, _agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        Box::pin(std::future::ready(ReadOutcome::MethodMissing))
    }

    fn probe_models(
        &self,
        _agent: AgentKind,
    ) -> Pin<Box<dyn Future<Output = Option<AgentModels>> + Send>> {
        let (models, probes) = (self.models.clone(), Arc::clone(&self.probes));
        Box::pin(async move {
            probes.fetch_add(1, Ordering::SeqCst);
            models
        })
    }
}

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

/// Claude's models as the probe finds them: Opus 5.5 at Medium by default, Fable 5.2, Haiku 5.5.
fn probed() -> AgentModels {
    let levels = |list: &[(&str, &str)]| list.iter().map(|(id, name)| named(id, name)).collect();
    let model = |id: &str, name: &str, efforts: Vec<NamedChoice>| ModelEntry {
        id: id.into(),
        name: name.into(),
        description: None,
        default_effort: (!efforts.is_empty()).then(|| "medium".into()),
        efforts,
    };
    let opus = levels(&[
        ("low", "Low"),
        ("medium", "Medium"),
        ("high", "High"),
        ("max", "Max"),
    ]);
    let fable = levels(&[("low", "Low"), ("medium", "Medium"), ("high", "High")]);
    AgentModels {
        model_option: "model".into(),
        effort_option: Some("effort".into()),
        models: vec![
            model("opus", "Opus 5.5", opus),
            model("fable", "Fable 5.2", fable),
            model("haiku", "Haiku 5.5", Vec::new()),
        ],
        current_model: "opus".into(),
        current_effort: Some("medium".into()),
    }
}

fn start(dir: &Dir, run: u16, models: Option<AgentModels>) -> (Running, Arc<AtomicUsize>) {
    let probes = Arc::new(AtomicUsize::new(0));
    let reader = ProbeReader {
        models,
        probes: Arc::clone(&probes),
    };
    let host = dir.start_with(run, |config| config.with_limits_reader(Arc::new(reader)));
    (host, probes)
}

/// Asks `ListAgents` until `done` holds of Claude's models, for at most ten seconds.
fn models_when(host: &Running, done: impl Fn(Option<&AgentModels>) -> bool) -> Option<AgentModels> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let models = agent_summary(host, CLAUDE).models;
        if done(models.as_ref()) || Instant::now() > until {
            return models;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_fresh_folder_names_the_real_models_after_the_probe() {
    let dir = Dir::new(
        "pickers_probe_fresh",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, probes) = start(&dir, 0, Some(probed()));
    let models = models_when(&host, |models| models.is_some()).expect("the probe was kept");
    assert_eq!(models, probed());
    let opus = models.model(&models.current_model).unwrap();
    assert_eq!(opus.name, "Opus 5.5", "the default by its real name");
    let effort = models.current_effort.as_deref().unwrap();
    let effort = opus
        .efforts
        .iter()
        .find(|level| level.id == effort)
        .unwrap();
    assert_eq!(effort.name, "Medium");
    // The options a session would report, so a pick before the first message is checked.
    let options = agent_summary(&host, CLAUDE).config_options;
    assert_eq!(options.len(), 2);
    assert_eq!(probes.load(Ordering::SeqCst), 1, "one probe per start");
    host.shutdown();
}

#[test]
fn a_failed_probe_keeps_the_kept_models() {
    let dir = Dir::new(
        "pickers_probe_failed",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, _) = start(&dir, 0, Some(probed()));
    models_when(&host, |models| models.is_some()).expect("the probe was kept");
    host.shutdown();
    let (host, probes) = start(&dir, 1, None);
    let until = Instant::now() + Duration::from_secs(10);
    while probes.load(Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(probes.load(Ordering::SeqCst), 1, "the probe ran");
    assert_eq!(agent_summary(&host, CLAUDE).models, Some(probed()));
    host.shutdown();
}

/// Every `TurnStarted` of `thread`'s stream so far.
fn started(events: &crate::observe::Events) -> Vec<TurnStarted> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::TurnStarted(started) => Some(started.clone()),
            _ => None,
        })
        .collect()
}

fn send_with(host: &Running, thread: ThreadId, prompt: &str, config: &[(&str, &str)]) {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let outcome = host.ok(HostCommand::SendPrompt {
        thread,
        content: text(prompt),
        delivery: DeliveryRequest::Queue,
        config: Some(picks(config)),
    });
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    events.until_turn_ended(turn);
    events.assert_no_mismatch();
}

#[test]
fn probed_picks_are_set_before_their_prompts_and_each_turn_names_its_model() {
    let dir = Dir::new(
        "pickers_probe_picks",
        &[(CLAUDE, Scenario::Bundled("pickers-probed"))],
    );
    let (host, _) = start(&dir, 0, Some(probed()));
    models_when(&host, |models| models.is_some()).expect("the probe was kept");
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    // A pick of the probed Fable before the first message: set at the launch.
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config: Some(picks(&[("model", "fable")])),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    let events = turn(&host, thread, "Summarise clause 4.");
    let first = started(&events);
    assert_eq!(first[0].model, Some(named("fable", "Fable 5.2")));
    assert_eq!(first[0].effort, Some(named("medium", "Medium")));
    // The second prompt's own pick: set just before it, named in its start.
    send_with(&host, thread, "Now clause 5.", &[("effort", "high")]);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let both = started(&events);
    assert_eq!(both.len(), 2, "{}", events.dump());
    assert_eq!(both[1].model, Some(named("fable", "Fable 5.2")));
    assert_eq!(both[1].effort, Some(named("high", "High")));
    let row = host.threads(matter).pop().unwrap();
    assert_eq!(row.model, Some(named("fable", "Fable 5.2")));
    assert_eq!(row.effort, Some(named("high", "High")));
    assert!(!row.code_mode);
    host.shutdown();
}
