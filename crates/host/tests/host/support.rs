//! The harness every test uses: a folder of its own under the temp folder, a `settings.json`
//! that maps agent ids to `fake_agent` scenarios, and a host started on it with a
//! `ManualClock`, `SequentialIds` and a `PassthroughLauncher` whose home is `<tmp>/home`.
//!
//! The helpers are blocking: each runs one future to completion with [`wait`], under a
//! watchdog of real time so a broken host fails the test instead of hanging it. They never nest
//! (a `block_on` inside a `block_on` panics), so tests call them only from plain code.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clauseo_host::{Host, HostConfig, HostHandle, HostJoin, ManualClock, SequentialIds};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    AgentId, CommandEnvelope, CommandId, CommandOutcome, Cursor, DeliveryRequest, ErrorCode,
    HostCommand, HostError, HostQuery, MatterId, MatterSource, MatterSummary, MessageId, Principal,
    PromptBlock, QueryResult, Seq, StreamId, ThreadId, ThreadStatus, ThreadSummary, Timestamp,
    Title, TurnId, ViewEpoch,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::future::Either;

use crate::observe::{Events, View};
use crate::write;

/// The wall time each run's clock starts at; a restart adds an hour per run.
pub(crate) const START_MS: i64 = 1_790_000_000_000;

/// How long a single wait on the host may take in real time before the test fails. Nothing
/// the host does in these tests takes more than a fraction of it.
pub(crate) const WATCHDOG: Duration = Duration::from_secs(20);

/// The agent id most tests use.
pub(crate) const FAKE: &str = "fake-agent";

/// Runs `fut` to completion, or returns `None` once [`WATCHDOG`] of real time has passed.
pub(crate) fn within<T>(fut: impl Future<Output = T>) -> Option<T> {
    futures::executor::block_on(async {
        let fut = std::pin::pin!(fut);
        let timer = async_io::Timer::after(WATCHDOG);
        match futures::future::select(fut, timer).await {
            Either::Left((value, _timer)) => Some(value),
            Either::Right(_) => None,
        }
    })
}

/// Runs `fut` to completion; fails the test naming `what` when the watchdog fires.
pub(crate) fn wait<T>(what: &str, fut: impl Future<Output = T>) -> T {
    within(fut).unwrap_or_else(|| panic!("timed out after {WATCHDOG:?} waiting for {what}"))
}

/// A scenario for an agent id: a bundled one by name, or a file in `tests/scenarios/`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Scenario {
    /// A scenario bundled with `fake_agent`, such as `prompt-stream`.
    Bundled(&'static str),
    /// `crates/host/tests/scenarios/<name>.jsonl`.
    Local(&'static str),
}

impl Scenario {
    /// The `--scenario` argument.
    fn arg(self) -> String {
        match self {
            Self::Bundled(name) => name.to_owned(),
            Self::Local(name) => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("scenarios")
                .join(format!("{name}.jsonl"))
                .display()
                .to_string(),
        }
    }
}

/// A test's own folder, `<temp>/clauseo-host-<test>-<pid>`, removed when dropped. The data
/// folder is the root itself (`Locations::under_root`), agents get `home/` as their home,
/// and matter folders go under `work/`.
#[derive(Debug)]
pub(crate) struct Dir {
    root: PathBuf,
}

impl Dir {
    /// A fresh folder for `test`, with `agents` written into `settings.json`.
    pub(crate) fn new(test: &str, agents: &[(&str, Scenario)]) -> Self {
        let root = std::env::temp_dir().join(format!("clauseo-host-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root.join("home"));
        write::create_dir_all(&root.join("work"));
        let dir = Self { root };
        dir.set_agents(agents);
        dir
    }

    /// Writes `settings.json` mapping each id to `fake_agent --scenario <scenario>`. Read at
    /// the next start.
    pub(crate) fn set_agents(&self, agents: &[(&str, Scenario)]) {
        let bin = clauseo_fake_agent::bin_path()
            .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
        let mut map = serde_json::Map::new();
        for (id, scenario) in agents {
            map.insert(
                (*id).to_owned(),
                serde_json::json!({
                    "command": bin.display().to_string(),
                    "args": ["--scenario", scenario.arg()],
                }),
            );
        }
        let settings = serde_json::json!({ "agents": map });
        write::write(
            &self.root.join("settings.json"),
            serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
        );
    }

    /// Makes `work/<name>` holding `files` (relative path, content) and returns its path.
    pub(crate) fn folder(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let folder = self.root.join("work").join(name);
        write::create_dir_all(&folder);
        for (path, content) in files {
            write::write(&folder.join(path), content.as_bytes());
        }
        folder
    }

    /// The config of run `run` (its clock starts `run` hours after [`START_MS`]).
    pub(crate) fn config(&self, run: u16, clock: &ManualClock) -> HostConfig {
        let locations = Locations::under_root(&self.root);
        let effective = Effective::load(&locations, &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new().home(self.root.join("home")));
        HostConfig::new(effective, Arc::new(launcher))
            .with_clock(Arc::new(clock.clone()))
            .with_ids(Box::new(SequentialIds::with_run(run)))
    }

    /// The clock of run `run`.
    pub(crate) fn clock(run: u16) -> ManualClock {
        let start = START_MS + i64::from(run) * 3_600_000;
        ManualClock::new(Timestamp::new(start).unwrap())
    }

    /// Starts run `run` with every default.
    pub(crate) fn start(&self, run: u16) -> Running {
        self.start_with(run, |config| config)
    }

    /// Starts run `run`, letting `adjust` change the config first.
    pub(crate) fn start_with(
        &self,
        run: u16,
        adjust: impl FnOnce(HostConfig) -> HostConfig,
    ) -> Running {
        let clock = Self::clock(run);
        let (handle, join) = Host::start(adjust(self.config(run, &clock)))
            .unwrap_or_else(|e| panic!("Host::start (run {run}): {e}"));
        Running {
            handle,
            join: Some(join),
            clock,
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        if let Err(error) = write::remove_dir_all(&self.root) {
            eprintln!("could not remove {}: {error}", self.root.display());
        }
    }
}

/// A fresh command id; never the same twice in this process, whatever the run.
pub(crate) fn command_id() -> CommandId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    CommandId::new(uuid::Uuid::from_u128((0xc0de_u128 << 64) | u128::from(n))).unwrap()
}

/// A valid v7 UUID no host run mints (its random bits are all ones), for ids the host does
/// not know.
pub(crate) fn foreign_uuid(n: u8) -> uuid::Uuid {
    let mut bytes = [0xff_u8; 10];
    bytes[9] = n;
    uuid::Builder::from_unix_timestamp_millis(1_700_000_000_000, &bytes).into_uuid()
}

/// A text prompt.
pub(crate) fn text(text: &str) -> Vec<PromptBlock> {
    vec![PromptBlock::Text {
        text: text.to_owned(),
    }]
}

/// A running host and the clock it reads.
pub(crate) struct Running {
    pub(crate) handle: HostHandle,
    join: Option<HostJoin>,
    pub(crate) clock: ManualClock,
}

impl Running {
    /// Dispatches `command` under a fresh id.
    pub(crate) fn dispatch(&self, command: HostCommand) -> Result<CommandOutcome, HostError> {
        self.dispatch_as(command_id(), command)
    }

    /// Dispatches `command` under `id`.
    pub(crate) fn dispatch_as(
        &self,
        id: CommandId,
        command: HostCommand,
    ) -> Result<CommandOutcome, HostError> {
        let envelope = CommandEnvelope {
            command_id: id,
            command,
        };
        wait("dispatch", self.handle.dispatch(envelope))
    }

    /// Dispatches `command`, which must succeed.
    pub(crate) fn ok(&self, command: HostCommand) -> CommandOutcome {
        let what = format!("{command:?}");
        self.dispatch(command)
            .unwrap_or_else(|e| panic!("{what} failed: {e}"))
    }

    /// Dispatches `command`, which must fail with `code`; returns the error.
    pub(crate) fn refused(&self, command: HostCommand, code: ErrorCode) -> HostError {
        match self.dispatch(command) {
            Ok(outcome) => panic!("expected {code:?}, got {outcome:?}"),
            Err(error) => {
                assert_eq!(error.code(), code, "{error}");
                error
            }
        }
    }

    /// Runs `query` as the local desktop.
    pub(crate) fn query(&self, query: HostQuery) -> Result<QueryResult, HostError> {
        wait("query", self.handle.query(Principal::LocalDesktop, query))
    }

    /// Imports `folder` as a matter named `name`.
    pub(crate) fn create_matter(&self, name: &str, folder: &Path) -> MatterId {
        let folder_ref = wait(
            "register_folder",
            self.handle.register_folder(folder.to_owned()),
        )
        .unwrap();
        let outcome = self.ok(HostCommand::CreateMatter {
            name: Title::new(name).unwrap(),
            source: MatterSource::ExistingFolder { folder: folder_ref },
        });
        match outcome {
            CommandOutcome::MatterCreated { matter } => matter,
            other => panic!("CreateMatter answered {other:?}"),
        }
    }

    /// A thread of `agent` in `matter`.
    pub(crate) fn create_thread(&self, matter: MatterId, agent: &str) -> ThreadId {
        let outcome = self.ok(HostCommand::CreateThread {
            matter,
            agent: AgentId::new(agent).unwrap(),
        });
        match outcome {
            CommandOutcome::ThreadCreated { thread } => thread,
            other => panic!("CreateThread answered {other:?}"),
        }
    }

    /// Sends a text prompt; returns its turn and message.
    pub(crate) fn prompt(&self, thread: ThreadId, prompt: &str) -> (TurnId, MessageId) {
        let outcome = self.ok(send_prompt(thread, prompt));
        match outcome {
            CommandOutcome::PromptAccepted { turn, message } => (turn, message),
            other => panic!("SendPrompt answered {other:?}"),
        }
    }

    /// The matters the host lists.
    pub(crate) fn matters(&self) -> Vec<MatterSummary> {
        match self.query(HostQuery::ListMatters).unwrap() {
            QueryResult::Matters(matters) => matters,
            other => panic!("ListMatters answered {other:?}"),
        }
    }

    /// The threads of `matter`.
    pub(crate) fn threads(&self, matter: MatterId) -> Vec<ThreadSummary> {
        match self.query(HostQuery::ListThreads { matter }).unwrap() {
            QueryResult::Threads(threads) => threads,
            other => panic!("ListThreads answered {other:?}"),
        }
    }

    /// The status `ListThreads` gives `thread`.
    pub(crate) fn status(&self, matter: MatterId, thread: ThreadId) -> ThreadStatus {
        self.threads(matter)
            .into_iter()
            .find(|summary| summary.thread == thread)
            .map_or_else(
                || panic!("thread {thread} not listed"),
                |summary| summary.status,
            )
    }

    /// A round trip through the host, so everything it published before is in the channels.
    pub(crate) fn settle(&self) {
        drop(self.matters());
    }

    /// An event subscription of `stream` after `after` (from the first when `None`).
    pub(crate) fn events_after(&self, stream: StreamId, after: Option<Seq>) -> Events {
        let sub = wait(
            "subscribe_events",
            self.handle
                .subscribe_events(Principal::LocalDesktop, stream, after),
        )
        .unwrap_or_else(|e| panic!("subscribe_events: {e}"));
        Events::new(sub)
    }

    /// An event subscription of `stream` from its first event.
    pub(crate) fn events(&self, stream: StreamId) -> Events {
        self.events_after(stream, None)
    }

    /// A view subscription of `stream`.
    pub(crate) fn view(&self, stream: StreamId, after: Option<(ViewEpoch, Cursor)>) -> View {
        let sub = wait(
            "subscribe",
            self.handle
                .subscribe(Principal::LocalDesktop, stream, after),
        )
        .unwrap_or_else(|e| panic!("subscribe: {e}"));
        View::new(sub)
    }

    /// Takes delays the clock armed until one equals `delay` (at most 64 of them).
    pub(crate) fn wait_armed(&self, delay: Duration) {
        let clock = self.clock.clone();
        let seen = wait(&format!("a {delay:?} timer"), async move {
            let mut seen = Vec::new();
            while seen.len() < 64 {
                let next = clock.next_armed().await;
                seen.push(next);
                if next == delay {
                    break;
                }
            }
            seen
        });
        assert_eq!(seen.last(), Some(&delay), "armed delays: {seen:?}");
    }

    /// Shuts the host down and joins its thread; both must succeed.
    pub(crate) fn shutdown(mut self) {
        wait("shutdown", self.handle.shutdown()).unwrap_or_else(|e| panic!("shutdown: {e}"));
        if let Some(join) = self.join.take() {
            join.join().unwrap_or_else(|e| panic!("join: {e}"));
        }
    }

    /// Shuts the host down while moving the clock past the 5 s cancel fallback when it is
    /// armed, for a running turn whose agent never answers its cancel.
    pub(crate) fn shutdown_past_cancel(mut self) {
        let clock = self.clock.clone();
        let mover = async move {
            while clock.next_armed().await != Duration::from_secs(5) {}
            clock.advance(Duration::from_secs(5));
        };
        let result = wait("shutdown", async {
            let shutdown = std::pin::pin!(self.handle.shutdown());
            let mover = std::pin::pin!(mover);
            match futures::future::select(shutdown, mover).await {
                Either::Left((result, _mover)) => result,
                Either::Right(((), shutdown)) => shutdown.await,
            }
        });
        result.unwrap_or_else(|e| panic!("shutdown: {e}"));
        if let Some(join) = self.join.take() {
            join.join().unwrap_or_else(|e| panic!("join: {e}"));
        }
    }

    /// Ends the host thread as a crash would, with no flush.
    pub(crate) fn abort(mut self) {
        wait("abort_for_test", self.handle.abort_for_test());
        if let Some(join) = self.join.take() {
            drop(join.join());
        }
    }

    /// The thread's join handle, for a test that checks it.
    pub(crate) fn take_join(&mut self) -> HostJoin {
        self.join.take().unwrap()
    }
}

impl Drop for Running {
    /// A test that ends without stopping its host (a failed assert) aborts it, so its agents'
    /// pipes close and the folder can be removed.
    fn drop(&mut self) {
        if let Some(join) = self.join.take()
            && within(self.handle.abort_for_test()).is_some()
        {
            drop(join.join());
        }
    }
}

/// `SendPrompt` with a text prompt and the default delivery.
pub(crate) fn send_prompt(thread: ThreadId, prompt: &str) -> HostCommand {
    HostCommand::SendPrompt {
        thread,
        content: text(prompt),
        delivery: DeliveryRequest::Queue,
    }
}

/// A matter imported from `work/<name>` (seeded with `files`) with one thread of `agent`.
pub(crate) fn matter_with_thread(
    dir: &Dir,
    host: &Running,
    name: &str,
    files: &[(&str, &str)],
    agent: &str,
) -> (MatterId, ThreadId, PathBuf) {
    let folder = dir.folder(name, files);
    let matter = host.create_matter(name, &folder);
    let thread = host.create_thread(matter, agent);
    (matter, thread, folder)
}
