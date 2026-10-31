//! A host driven one input at a time on the test's own thread: the actor from
//! `startup::open` with a real store in a temp folder, the manual clock, sequential ids, and a
//! `fake_agent` scenario written into the folder. A test can hold inputs back (a commit's
//! result, an agent's end) to put the actor in the order it wants.

use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentSessionAttached, CheckpointId, CheckpointRecorded,
    CommandEnvelope, CommandId, CommandOutcome, Delivery, DomainEvent, Envelope, HostCommand,
    MatterId, MatterKind, StreamId, ThreadId, Timestamp, TurnId, TurnStarted, UserMessage,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::channel::oneshot;
use futures::executor::LocalPool;
use futures::future::Either;
use uuid::Uuid;

use super::super::super::input::{Input, Request};
use super::super::super::{Actor, Phase as HostPhase, shutdown, startup, streams};
use super::super::out::Out;
use super::super::{Current, Phase, Then};
use crate::error::Error;
use crate::pipeline::tests::support::TempDir;
use crate::{HostConfig, ManualClock, SequentialIds};

/// How long one wait for an input may take in real time before the test fails.
const WATCHDOG: Duration = Duration::from_secs(20);

/// The handshake every scenario opens with.
pub(super) const HANDSHAKE: &str = r#"{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1}}}
{"respond":{"to":"init","result":{"protocolVersion":1,"agentCapabilities":{"loadSession":false},"authMethods":[],"agentInfo":{"name":"fake_agent","version":"0.0.0"}}}}
{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}}
{"respond":{"to":"new","result":{"sessionId":"sess-1"}}}
"#;

/// The command id of the thread's `CreateThread`, whose receipt the store holds.
pub(super) const STORED: u128 = 1;

/// A command id of the tests.
pub(super) fn command_id(n: u128) -> CommandId {
    CommandId::new(Uuid::from_u128(n)).unwrap()
}

pub(super) struct Rig {
    pub(super) actor: Actor,
    pool: LocalPool,
    internal: async_channel::Receiver<Input>,
    /// Inputs taken off the channel and held back.
    pub(super) held: Vec<Input>,
    pub(super) clock: ManualClock,
    _dir: TempDir,
}

impl Rig {
    /// A started host whose agent `fake-agent` runs `steps` after the handshake.
    pub(super) fn start(test: &str, steps: &str) -> Self {
        let dir = TempDir::new(test);
        let root = dir.path();
        crate::write::create_dir_all(&root.join("home")).unwrap();
        let scenario = root.join(format!("{test}.jsonl"));
        let header = format!(
            "{{\"scenario\":\"{test}\",\"description\":\"a unit test\",\"on_stdin_close\":\"exit\"}}\n"
        );
        let text = format!("{header}{HANDSHAKE}{steps}");
        crate::write::write_file(&scenario, text.as_bytes()).unwrap();
        let bin = clauseo_fake_agent::bin_path().expect("run cargo build -p clauseo-fake_agent");
        let settings = serde_json::json!({ "agents": { "fake-agent": {
            "command": bin.display().to_string(),
            "args": ["--scenario", scenario.display().to_string()],
        }}});
        crate::write::write_file(&root.join("settings.json"), settings.to_string().as_bytes())
            .unwrap();
        let effective = Effective::load(&Locations::under_root(root), &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new().home(root.join("home")));
        let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
        let config = HostConfig::new(effective, Arc::new(launcher))
            .with_clock(Arc::new(clock.clone()))
            .with_ids(Box::new(SequentialIds::new()));
        let mut pool = LocalPool::new();
        let (post, internal) = async_channel::unbounded();
        let (ready, _ready) = std::sync::mpsc::channel();
        let open = startup::open(config, pool.spawner(), post, ready);
        let mut actor = pool.run_until(open).unwrap();
        startup::begin_startup(&mut actor);
        let mut rig = Self {
            actor,
            pool,
            internal,
            held: Vec::new(),
            clock,
            _dir: dir,
        };
        rig.run_until("start-up", |actor| {
            matches!(actor.phase, HostPhase::Serving)
        });
        rig
    }

    /// The Scratch matter.
    pub(super) fn matter(&self) -> MatterId {
        self.actor
            .matters
            .values()
            .find(|matter| matter.row.kind == MatterKind::Scratch)
            .map(|matter| matter.row.id)
            .unwrap()
    }

    /// The next input, under the watchdog.
    fn next(&mut self) -> Input {
        let recv = self.internal.recv();
        let waited = self.pool.run_until(async {
            let recv = std::pin::pin!(recv);
            match futures::future::select(recv, async_io::Timer::after(WATCHDOG)).await {
                Either::Left((input, _)) => input.ok(),
                Either::Right(_) => None,
            }
        });
        waited.unwrap_or_else(|| panic!("no input for {WATCHDOG:?}"))
    }

    /// Handles inputs until `done` holds, holding back those `hold` picks.
    pub(super) fn run_holding(
        &mut self,
        what: &str,
        mut hold: impl FnMut(&Actor, &Input) -> bool,
        mut done: impl FnMut(&mut Actor) -> bool,
    ) {
        for _ in 0..100_000 {
            let _exit = self.actor.settle();
            if done(&mut self.actor) {
                return;
            }
            let input = self.next();
            if hold(&self.actor, &input) {
                self.held.push(input);
            } else {
                self.actor.on_input(input);
            }
        }
        panic!("never reached {what}");
    }

    /// Handles inputs until `hold` picks one, which is held back.
    pub(super) fn hold_next(&mut self, what: &str, mut hold: impl FnMut(&Actor, &Input) -> bool) {
        for _ in 0..100_000 {
            let _exit = self.actor.settle();
            let input = self.next();
            if hold(&self.actor, &input) {
                self.held.push(input);
                return;
            }
            self.actor.on_input(input);
        }
        panic!("never held {what}");
    }

    /// Handles inputs until `done` holds.
    pub(super) fn run_until(&mut self, what: &str, done: impl FnMut(&mut Actor) -> bool) {
        self.run_holding(what, |_, _| false, done);
    }

    /// Handles the inputs held back, in the order they came.
    pub(super) fn release_held(&mut self) {
        for input in std::mem::take(&mut self.held) {
            self.actor.on_input(input);
        }
    }

    /// Handles inputs until no commit is in flight.
    pub(super) fn settle_commits(&mut self) {
        self.run_until("no commit in flight", |actor| actor.work.commits == 0);
    }

    /// Dispatches `command` as command `n` and handles inputs until it is answered.
    pub(super) fn dispatch(
        &mut self,
        n: u128,
        command: HostCommand,
    ) -> Result<CommandOutcome, Error> {
        let (reply, mut answer) = oneshot::channel();
        let command = CommandEnvelope {
            command_id: command_id(n),
            command,
        };
        self.actor.on_request(Request::Dispatch { command, reply });
        let mut got = None;
        self.run_until("the command's answer", |_| {
            if let Ok(Some(result)) = answer.try_recv() {
                got = Some(result);
            }
            got.is_some()
        });
        got.unwrap()
    }

    /// A new thread of the Scratch matter, made by command [`STORED`].
    pub(super) fn thread(&mut self) -> ThreadId {
        self.thread_by(STORED)
    }

    /// A new thread of the Scratch matter, made by command `n`.
    pub(super) fn thread_by(&mut self, n: u128) -> ThreadId {
        let command = HostCommand::CreateThread {
            matter: self.matter(),
            agent: AgentId::new("fake-agent").unwrap(),
        };
        let created = self.dispatch(n, command).unwrap();
        let CommandOutcome::ThreadCreated { thread } = created else {
            panic!("not a thread: {created:?}");
        };
        thread
    }

    /// A turn of `thread` whose prompt is in flight, with no agent: its stream loaded, and
    /// `AgentSessionAttached`, `UserMessage` and `TurnStarted` durable.
    pub(super) fn running_turn(&mut self, thread: ThreadId) -> TurnId {
        let now = self.actor.now().wall;
        let turn: TurnId = self.actor.ids.mint(now).unwrap();
        let mut current = Current::new(turn, Vec::new());
        current.phase = Phase::Prompting;
        self.actor.threads.get_mut(&thread).unwrap().turn.current = Some(Box::new(current));
        let stream = StreamId::Thread(thread);
        self.actor
            .with_loaded(stream, Box::new(|_, loaded| loaded.unwrap()));
        self.run_until("the thread loaded", |actor| actor.loaded(stream).is_some());
        let events: Vec<DomainEvent> = vec![
            AgentSessionAttached {
                session: "sess-1".parse().unwrap(),
                agent_version: None,
                capabilities: AgentCapabilities::default(),
            }
            .into(),
            UserMessage {
                turn,
                message: self.actor.ids.mint(now).unwrap(),
                acp_message_id: None,
                content: Vec::new(),
                delivery: Delivery::Queue,
            }
            .into(),
            TurnStarted {
                turn,
                checkpoint_before: self.last_recorded(),
            }
            .into(),
        ];
        self.commit(thread, events);
        self.settle_commits();
        turn
    }

    /// Sends one commit of `events` in `thread`'s stream, built as the turn path builds one.
    pub(super) fn commit(&mut self, thread: ThreadId, events: Vec<DomainEvent>) {
        let actor = &mut self.actor;
        let mut out = Out::new(actor, thread, None).unwrap();
        for event in events {
            out.push(actor, event).unwrap();
        }
        out.send(actor, Then::Nothing).unwrap();
    }

    /// Makes the store refuse every commit `pick` chooses, up to `times` of them.
    pub(super) fn fail(&mut self, times: usize, pick: impl Fn(&DomainEvent) -> bool + 'static) {
        let mut left = times;
        self.actor.fault = Some(Box::new(move |events: &[Envelope]| {
            let picked = left > 0 && events.iter().any(|envelope| pick(&envelope.event));
            picked.then(|| {
                left -= 1;
                command_id(STORED)
            })
        }));
    }

    /// Every stored event of `thread`'s stream.
    pub(super) fn log(&mut self, thread: ThreadId) -> Vec<DomainEvent> {
        let store = self.actor.store.clone();
        let read = streams::read_whole_log(&store, StreamId::Thread(thread));
        let reopen = self.pool.run_until(read).unwrap();
        reopen
            .tail
            .into_iter()
            .map(|envelope| envelope.event)
            .collect()
    }

    /// Every stored event of the Scratch matter's stream.
    pub(super) fn matter_log(&mut self) -> Vec<DomainEvent> {
        let store = self.actor.store.clone();
        let read = streams::read_whole_log(&store, StreamId::Matter(self.matter()));
        let reopen = self.pool.run_until(read).unwrap();
        reopen
            .tail
            .into_iter()
            .map(|envelope| envelope.event)
            .collect()
    }

    /// The checkpoints the Scratch matter's log records, in order.
    pub(super) fn recorded(&mut self) -> Vec<CheckpointRecorded> {
        self.matter_log()
            .into_iter()
            .filter_map(|event| {
                if let DomainEvent::CheckpointRecorded(recorded) = event {
                    Some(recorded)
                } else {
                    None
                }
            })
            .collect()
    }

    /// The last checkpoint the Scratch matter's log records.
    pub(super) fn last_recorded(&mut self) -> CheckpointId {
        self.recorded().last().unwrap().checkpoint()
    }

    /// Writes `text` to `name` in the Scratch matter's folder.
    pub(super) fn write_file(&self, name: &str, text: &str) {
        let matter = &self.actor.matters[&self.matter()];
        let path = std::path::Path::new(&matter.row.folder_path).join(name);
        crate::write::write_file(&path, text.as_bytes()).unwrap();
    }

    /// Shuts the host down and closes the store.
    pub(super) fn finish(mut self) {
        shutdown::begin_shutdown(&mut self.actor, None);
        // A cancelled turn with no agent to answer ends at its fallback.
        self.clock.advance(super::super::CANCEL_WAIT);
        loop {
            if self.actor.settle().is_some() {
                break;
            }
            let input = self.next();
            self.actor.on_input(input);
        }
        let Self {
            actor, mut pool, ..
        } = self;
        pool.run_until(shutdown::close(actor)).unwrap();
    }
}

/// The name of an event's kind, for asserts.
pub(super) fn kind(event: &DomainEvent) -> String {
    let json = serde_json::to_value(event).unwrap();
    json.as_object()
        .and_then(|object| object.keys().next().cloned())
        .unwrap_or_else(|| json.to_string())
}
