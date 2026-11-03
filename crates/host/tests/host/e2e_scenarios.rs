//! Every `fake_agent` scenario the window's end-to-end specs (`ui/e2e/app-windows/*.spec.ts`)
//! play as `agents.claude-acp` launches as the desktop app launches it (a Claude agent, the
//! pre-write guard set) and answers the first prompt. The list is read from the specs, so a new
//! spec's scenario is checked too: a scenario that misses the Claude launch (no modes, or none
//! in `default`, in its `session/new` answer) fails here, not on the Windows runner.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::ManualClock;
use clauseo_protocol::{DomainEvent, HostCommand, StreamId};
use futures::channel::oneshot;
use futures::future::Either;

use crate::observe::Events;

use crate::support::{Dir, Scenario, matter_with_thread};

/// The agent id the specs' `settings.json` gives `fake_agent` (`scratch.ts`).
const CLAUDE: &str = "claude-acp";

/// The matter files the specs seed (`matterFiles`), all in every matter: files-one-turn's
/// `BEFORE` and word-locked's `PETITION_BEFORE`.
const MATTER_FILES: &[(&str, &str)] = &[
    ("brief.txt", "Clause 4 draft\n"),
    ("old.txt", "Old terms\n"),
    ("Petition.docx", "Petition draft\n"),
];

/// The durability timer that commits streamed text.
const DURABLE: Duration = Duration::from_millis(250);

/// The cancel fallback.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// The program the desktop sets as the guard (its own executable); nothing runs it.
const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";

/// `ui/e2e/app-windows`.
fn specs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/e2e/app-windows")
}

/// The constants a spec passes as `scenario:` instead of a name, with the scenario each names:
/// the scoreboard's (`ui/e2e/scoreboard/session.ts`) and word-locked's file.
fn constant(name: &str) -> Option<Scenario> {
    match name {
        "SCENARIO" => Some(Scenario::Bundled("scoreboard")),
        "WORD_SCENARIO" => Some(Scenario::Ui("word-one-docx")),
        _ => None,
    }
}

/// Every `scenario:` value in the specs, by spec, deduplicated by its argument.
fn spec_scenarios() -> Vec<(String, Scenario)> {
    let mut found: Vec<(String, Scenario)> = Vec::new();
    let mut specs: Vec<PathBuf> = std::fs::read_dir(specs_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".spec.ts"))
        .collect();
    specs.sort();
    for spec in specs {
        let text = std::fs::read_to_string(&spec).unwrap();
        for (at, _) in text.match_indices("scenario: ") {
            let rest = &text[at + "scenario: ".len()..];
            let value: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '"' | '-' | '_'))
                .collect();
            let scenario = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(name) => Scenario::Bundled(Box::leak(name.to_owned().into_boxed_str())),
                None => constant(&value).unwrap_or_else(|| {
                    panic!("{}: unknown scenario value {value:?}", spec.display())
                }),
            };
            if !found.iter().any(|(_, seen)| seen.arg() == scenario.arg()) {
                found.push((spec.display().to_string(), scenario));
            }
        }
    }
    found
}

/// The kinds that only follow the agent's work on a prompt.
fn answered(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::TextAppended(_)
            | DomainEvent::ContentAppended(_)
            | DomainEvent::ToolCallStarted(_)
            | DomainEvent::PermissionRequested(_)
    )
}

/// Moves the test's clock past each durability timer (streamed text commits on it) and each
/// 5 s cancel fallback as they are armed, until dropped.
struct Mover {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Mover {
    fn start(clock: ManualClock) -> Self {
        let (stop, mut stopped) = oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            futures::executor::block_on(async move {
                loop {
                    let next = std::pin::pin!(clock.next_armed());
                    match futures::future::select(next, &mut stopped).await {
                        Either::Left((delay, _)) if delay == DURABLE || delay == CANCEL_WAIT => {
                            clock.advance(delay);
                        }
                        Either::Left(_) => {}
                        Either::Right(_) => break,
                    }
                }
            });
        });
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Mover {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

/// Whether the thread's turn has ended.
fn ended(events: &Events) -> bool {
    events
        .events()
        .any(|event| matches!(event, DomainEvent::TurnEnded(_)))
}

#[test]
fn every_scenario_the_window_specs_play_launches_as_claude_and_answers() {
    let scenarios = spec_scenarios();
    assert!(scenarios.len() >= 10, "{} scenarios", scenarios.len());
    for (n, (spec, scenario)) in scenarios.into_iter().enumerate() {
        let arg = scenario.arg();
        let dir = Dir::new(&format!("e2e_scenarios_{n}"), &[(CLAUDE, scenario)]);
        let host = dir.start_with(0, |config| {
            config.with_pre_write_guard(PathBuf::from(PROGRAM))
        });
        let (_, thread, _) = matter_with_thread(&dir, &host, "m", MATTER_FILES, CLAUDE);
        let mut events = host.events(StreamId::Thread(thread));
        events.until_caught_up();
        host.prompt(thread, "Go");
        events.until(&format!("{arg} ({spec}): attached or failed"), |e| {
            e.events().any(|event| {
                matches!(
                    event,
                    DomainEvent::AgentSessionAttached(_)
                        | DomainEvent::TurnFailed(_)
                        | DomainEvent::TurnEnded(_)
                )
            })
        });
        assert!(
            events
                .events()
                .any(|event| matches!(event, DomainEvent::AgentSessionAttached(_))),
            "{arg} ({spec}) failed its launch:{}",
            events.dump()
        );
        let mover = Mover::start(host.clock.clone());
        events.until(&format!("{arg} ({spec}): a first answer"), |e| {
            e.events().any(answered)
        });
        events.assert_no_mismatch();
        // A turn still running (a permission asked, a cancel never answered) is cancelled and
        // reaches its end, by the 5 s fallback at the latest, before the host stops.
        drop(host.dispatch(HostCommand::Cancel { thread }));
        events.until(&format!("{arg} ({spec}): the turn's end"), ended);
        drop(mover);
        host.shutdown();
    }
}
