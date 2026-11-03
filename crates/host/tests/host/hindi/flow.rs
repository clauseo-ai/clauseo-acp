//! The Hindi question's flow through the host, the same for the fake agent and a live one:
//! the fixture papers copied into a fresh matter folder, a matter and a thread of the agent,
//! the agent's model (and effort) checked as it reports them and set with `SetConfigOption`
//! when it offers the wanted choice, then the question in Hindi with the four papers attached
//! as the composer's `@` mentions send them (file blocks), any permission request answered
//! allow-once, and the answer read from the thread's events.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::{EventSubscription, HostHandle};
use clauseo_protocol::{
    AgentChoiceId, AgentId, CommandEnvelope, CommandId, CommandOutcome, ConfigCategory,
    ConfigOption, ConfigOptionKind, ConfigValue, DeliveryRequest, DomainEvent, ErrorCode,
    HostCommand, HostError, HostEvent, MatterSource, MessageId, PermissionKind, Principal,
    PromptBlock, StreamId, TextKind, ThreadId, Title, TurnId,
};
use futures::StreamExt as _;
use futures::future::Either;

use super::verdict::{PAPERS, QUESTION, Verdict};
use crate::write;

/// What the flow asks of the agent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wanted {
    /// The settings agent id; `claude-acp` and `codex-acp` get their kind's launch.
    pub(crate) agent: &'static str,
    /// The model the agent must report (compared loosely: see [`names`]).
    pub(crate) model: &'static str,
    /// The reasoning effort it must report, when it reports one.
    pub(crate) effort: Option<&'static str>,
    /// How long one step may take in real time before the test fails.
    pub(crate) patience: Duration,
}

/// What came back.
#[derive(Debug)]
pub(crate) struct Asked {
    /// The answer's text (`TextAppended` of kind answer, joined).
    pub(crate) answer: String,
    /// The pass rule over it.
    pub(crate) verdict: Verdict,
    /// The model choice the agent reported last, as `value (name)`; `None` when it offers no
    /// model option.
    pub(crate) model: Option<String>,
    /// The effort choice it reported last; `None` when it offers no effort option.
    pub(crate) effort: Option<String>,
    /// The fixture papers its tool calls named (locations, titles or inputs).
    pub(crate) read: Vec<&'static str>,
    /// How many permission requests the flow answered allow-once.
    pub(crate) permissions: usize,
}

impl Asked {
    /// The record the live test prints.
    pub(crate) fn record(&self) -> String {
        format!(
            "answer:\n{}\n\nmodel: {}\neffort: {}\npapers its tools read: {:?}\npermissions \
             allowed: {}\n{}",
            self.answer,
            self.model.as_deref().unwrap_or("(no model option offered)"),
            self.effort
                .as_deref()
                .unwrap_or("(no effort option offered)"),
            self.read,
            self.permissions,
            self.verdict,
        )
    }
}

/// Runs the flow on `handle`, with the matter folder made in `work`. Fails the test when a
/// step fails or takes longer than `wanted.patience`, and when the agent reports a model (or
/// effort) other than the wanted one.
pub(crate) fn ask(handle: &HostHandle, work: &Path, wanted: Wanted) -> Asked {
    let host = Steps { handle, wanted };
    let folder = seed(work);
    let folder_ref = host.wait("register_folder", handle.register_folder(folder.clone()));
    let matter = match host.ok(HostCommand::CreateMatter {
        name: Title::new("हिंदी किरायानामा").unwrap(),
        source: MatterSource::ExistingFolder {
            folder: folder_ref.unwrap(),
        },
        client: None,
        next_date: None,
    }) {
        CommandOutcome::MatterCreated { matter } => matter,
        other => panic!("CreateMatter answered {other:?}"),
    };
    let thread = match host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(wanted.agent).unwrap(),
    }) {
        CommandOutcome::ThreadCreated { thread } => thread,
        other => panic!("CreateThread answered {other:?}"),
    };
    let mut events = Events {
        sub: host
            .wait(
                "subscribe_events",
                handle.subscribe_events(Principal::LocalDesktop, StreamId::Thread(thread), None),
            )
            .unwrap(),
        seen: Vec::new(),
    };
    let (model, effort) = host.choose(thread, &mut events);
    let files = PAPERS.map(|paper| {
        let file = host.wait("register_file", handle.register_file(folder.join(paper)));
        PromptBlock::File {
            file: file.unwrap(),
        }
    });
    let mut content = vec![PromptBlock::Text {
        text: QUESTION.to_owned(),
    }];
    content.extend(files);
    let turn = match host.ok(HostCommand::SendPrompt {
        thread,
        content,
        delivery: DeliveryRequest::Queue,
    }) {
        CommandOutcome::PromptAccepted { turn, .. } => turn,
        other => panic!("SendPrompt answered {other:?}"),
    };
    let permissions = host.answer_until_ended(thread, turn, &mut events);
    let answer = events.answer(turn);
    Asked {
        verdict: Verdict::of(&answer),
        answer,
        model,
        effort,
        read: events.papers_read(turn),
        permissions,
    }
}

/// Copies the fixture papers into `work/hindi-matter-<random>` and returns it: a new folder
/// each run, since the live data folder still lists the matters of earlier runs.
fn seed(work: &Path) -> PathBuf {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hindi");
    let id = uuid::Uuid::now_v7().simple().to_string();
    let folder = work.join(format!("hindi-matter-{}", &id[id.len() - 8..]));
    write::create_dir_all(&folder);
    for paper in PAPERS {
        let bytes = std::fs::read(fixtures.join(paper)).unwrap();
        write::write(&folder.join(paper), &bytes);
    }
    folder
}

struct Steps<'a> {
    handle: &'a HostHandle,
    wanted: Wanted,
}

impl Steps<'_> {
    /// Runs `fut` to completion, or fails the test naming `what` after the patience.
    fn wait<T>(&self, what: &str, fut: impl Future<Output = T>) -> T {
        let patience = self.wanted.patience;
        futures::executor::block_on(async {
            let fut = std::pin::pin!(fut);
            let timer = async_io::Timer::after(patience);
            match futures::future::select(fut, timer).await {
                Either::Left((value, _timer)) => value,
                Either::Right(_) => panic!("timed out after {patience:?} waiting for {what}"),
            }
        })
    }

    fn dispatch(&self, command: HostCommand) -> Result<CommandOutcome, HostError> {
        // A fresh v7 id, never a per-process count: the live data folder keeps every run's
        // receipts, and a repeated id is `command.conflict`.
        let envelope = CommandEnvelope {
            command_id: CommandId::new(uuid::Uuid::now_v7()).unwrap(),
            command,
        };
        self.wait("a command", self.handle.dispatch(envelope))
    }

    fn ok(&self, command: HostCommand) -> CommandOutcome {
        let what = format!("{command:?}");
        self.dispatch(command)
            .unwrap_or_else(|error| panic!("{what} failed: {error}"))
    }

    /// Starts the agent's session with a `SetConfigOption` of `model` to the wanted model (the
    /// host launches the agent for it), then checks the options the agent reports and sets
    /// any that is not the wanted choice. Returns the model and effort choices reported last.
    fn choose(&self, thread: ThreadId, events: &mut Events) -> (Option<String>, Option<String>) {
        let wanted = self.wanted;
        let first = self.dispatch(set(thread, "model", wanted.model));
        if let Err(error) = &first {
            assert_ne!(
                error.code(),
                ErrorCode::AgentAuthRequired,
                "{} is not signed in: {error}",
                wanted.agent
            );
            eprintln!("the first SetConfigOption was refused ({error}); reading the options");
        }
        let attached = |e: &Events| e.attached();
        events.until(self, "the agent's session", attached);
        events.drain_ready();
        let model = self.ensure(thread, events, ConfigCategory::Model, wanted.model);
        let effort = wanted
            .effort
            .and_then(|effort| self.ensure(thread, events, ConfigCategory::ThoughtLevel, effort));
        (model, effort)
    }

    /// The current choice of the option of `category`, after setting it to the choice that
    /// names `want` when it is not that already. `None` when the agent offers no such option;
    /// fails the test when it offers one with no choice naming `want`.
    fn ensure(
        &self,
        thread: ThreadId,
        events: &mut Events,
        category: ConfigCategory,
        want: &str,
    ) -> Option<String> {
        let option = events.option(category)?;
        let ConfigOptionKind::Select {
            current_value,
            options,
        } = &option.kind
        else {
            panic!("the {category:?} option is not a select: {option:?}");
        };
        let shown = |value: &str| {
            options
                .iter()
                .find(|choice| choice.value == value)
                .map_or_else(
                    || value.to_owned(),
                    |choice| format!("{} ({})", choice.value, choice.name),
                )
        };
        let Some(choice) = options
            .iter()
            .find(|choice| names(&choice_text(choice), want))
        else {
            panic!(
                "the agent offers no {category:?} choice for {want}; it offers {:?}",
                options
                    .iter()
                    .map(|choice| &choice.value)
                    .collect::<Vec<_>>()
            );
        };
        if choice.value != *current_value {
            let id = option.id.to_string();
            let value = choice.value.to_string();
            self.ok(set(thread, &id, &value));
            events.drain_ready();
            let now = events
                .option(category)
                .and_then(|option| match option.kind {
                    ConfigOptionKind::Select { current_value, .. } => Some(current_value),
                    _ => None,
                });
            assert_eq!(
                now.as_deref(),
                Some(value.as_str()),
                "the agent did not take {value}"
            );
            return Some(shown(&value));
        }
        Some(shown(current_value))
    }

    /// Answers each permission request allow-once until `turn` ends; returns how many.
    fn answer_until_ended(&self, thread: ThreadId, turn: TurnId, events: &mut Events) -> usize {
        let mut answered = 0;
        loop {
            let from = events.seen.len();
            events.until(self, "the next event of the turn", |e| e.seen.len() > from);
            let new: Vec<DomainEvent> = events.seen[from..].to_vec();
            for event in new {
                match event {
                    DomainEvent::PermissionRequested(requested) => {
                        let allow = requested
                            .options
                            .iter()
                            .find(|option| option.kind == PermissionKind::AllowOnce)
                            .unwrap_or_else(|| panic!("no allow-once option: {requested:?}"));
                        eprintln!("allowing once: {:?}", allow.agent_label);
                        self.ok(HostCommand::AnswerPermission {
                            thread,
                            request: requested.request,
                            option_id: AgentChoiceId::new(allow.option_id.as_str()).unwrap(),
                        });
                        answered += 1;
                    }
                    DomainEvent::TurnEnded(ended) if ended.turn == turn => {
                        eprintln!("turn ended: {:?}", ended.stop);
                        return answered;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `SetConfigOption` of `option` to the select value `value`.
fn set(thread: ThreadId, option: &str, value: &str) -> HostCommand {
    HostCommand::SetConfigOption {
        thread,
        option_id: AgentChoiceId::new(option).unwrap(),
        value: ConfigValue::Select(AgentChoiceId::new(value).unwrap()),
    }
}

fn choice_text(choice: &clauseo_protocol::ConfigChoice) -> String {
    format!(
        "{} {} {}",
        choice.value,
        choice.name,
        choice.description.as_deref().unwrap_or("")
    )
}

/// Whether `text` names `want`: both lowercased with spaces, dots and underscores as hyphens,
/// so `claude-haiku-5-5` matches "Claude Haiku 5.5", and `gpt-6-luna` matches "GPT-6 Luna".
pub(crate) fn names(text: &str, want: &str) -> bool {
    let fold = |text: &str| {
        text.to_lowercase()
            .chars()
            .map(|c| if matches!(c, ' ' | '.' | '_') { '-' } else { c })
            .collect::<String>()
    };
    fold(text).contains(&fold(want))
}

/// The thread's events as they arrive.
struct Events {
    sub: EventSubscription,
    seen: Vec<DomainEvent>,
}

impl Events {
    fn take(&mut self, item: Option<HostEvent>) {
        match item {
            Some(HostEvent::Events(envelopes)) => {
                self.seen
                    .extend(envelopes.into_iter().map(|envelope| envelope.event));
            }
            Some(HostEvent::CaughtUp { .. }) => {}
            Some(other) => panic!("an event subscription delivered {other:?}"),
            None => panic!("the thread's stream ended; seen: {:#?}", self.seen),
        }
    }

    /// Reads until `done` holds, under `steps`' patience.
    fn until(&mut self, steps: &Steps<'_>, what: &str, done: impl Fn(&Self) -> bool) {
        steps.wait(what, async {
            while !done(self) {
                let item = self.sub.next().await;
                self.take(item);
            }
        });
    }

    /// Takes what is already in the channel, without waiting.
    fn drain_ready(&mut self) {
        while let Some(item) = futures::FutureExt::now_or_never(self.sub.next()) {
            self.take(item);
        }
    }

    fn attached(&self) -> bool {
        self.seen
            .iter()
            .any(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
    }

    /// The last reported option of `category`.
    fn option(&self, category: ConfigCategory) -> Option<ConfigOption> {
        self.seen.iter().rev().find_map(|event| match event {
            DomainEvent::ConfigOptionsReplaced(replaced) => replaced
                .options
                .iter()
                .find(|option| option.category == category)
                .cloned(),
            _ => None,
        })
    }

    /// `turn`'s answer text: each agent message's text (its `TextAppended` batches joined in
    /// order), with the messages in the order they began and a newline between them, so a
    /// working sentence before a tool call does not run into the next message's first line.
    fn answer(&self, turn: TurnId) -> String {
        let mut messages: Vec<(MessageId, String)> = Vec::new();
        for event in &self.seen {
            let DomainEvent::TextAppended(appended) = event else {
                continue;
            };
            if appended.turn != turn || appended.kind != TextKind::Answer {
                continue;
            }
            match messages
                .iter_mut()
                .find(|(message, _)| *message == appended.message)
            {
                Some((_, text)) => text.push_str(&appended.text),
                None => messages.push((appended.message, appended.text.clone())),
            }
        }
        messages
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The fixture papers that `turn`'s tool calls name in their locations, titles or inputs.
    fn papers_read(&self, turn: TurnId) -> Vec<&'static str> {
        let said: String = self
            .seen
            .iter()
            .filter_map(|event| match event {
                DomainEvent::ToolCallStarted(started) if started.turn == turn => {
                    Some(format!("{started:?}"))
                }
                _ => None,
            })
            .collect();
        PAPERS
            .into_iter()
            .filter(|paper| said.contains(paper))
            .collect()
    }
}
