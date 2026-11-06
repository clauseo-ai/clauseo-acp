//! The harness every test uses: a folder of its own under the temp folder, a `settings.json`
//! that maps agent ids to `fake_agent` scenarios, and a host started on it with a
//! `ManualClock`, `SequentialIds` and a `PassthroughLauncher` whose home is `<tmp>/home`. The
//! one policy source is `<root>/policy.json`, and the host looks for Claude Code's enterprise
//! `managed-mcp.json` at `<root>/managed-mcp.json`, never at the system path. Its limits
//! reader is [`NoReader`], which launches nothing: a `claude-acp` or `codex-acp` entry
//! would otherwise start the real readers at start-up. `limits_host.rs` sets its own.
//!
//! The helpers are blocking: each runs one future to completion with [`wait`], under a
//! watchdog of real time so a broken host fails the test instead of hanging it. They never nest
//! (a `block_on` inside a `block_on` panics), so tests call them only from plain code.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use clauseo_host::{Host, HostConfig, HostHandle, HostJoin, ManualClock, SequentialIds};
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    AgentId, AgentKind, CommandEnvelope, CommandId, CommandOutcome, Cursor, DeliveryRequest,
    ErrorCode, FolderRefId, HostCommand, HostError, HostQuery, MatterId, MatterSource,
    MatterSummary, MessageId, Principal, PromptBlock, QueryResult, Seq, StreamId, ThreadId,
    ThreadStatus, ThreadSummary, Timestamp, Title, TurnId, ViewEpoch,
};
use clauseo_settings::{Effective, FilePolicy, Locations, SystemFiles};
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

/// A scenario for an agent id: a bundled one by name, a file in `tests/scenarios/`, or one a
/// test wrote itself (for a path only known at run time).
#[derive(Debug, Clone)]
pub(crate) enum Scenario {
    /// A scenario bundled with `fake_agent`, such as `prompt-stream`.
    Bundled(&'static str),
    /// `crates/host/tests/scenarios/<name>.jsonl`.
    Local(&'static str),
    /// `ui/e2e/app-windows/scenarios/<name>.jsonl`, a scenario the window's specs play by path.
    Ui(&'static str),
    /// A scenario file the test wrote (the Unix symlink test, and the launch profile's tests,
    /// whose launcher fills in each session's folders).
    Written(PathBuf),
}

impl Scenario {
    /// The `--scenario` argument.
    pub(crate) fn arg(&self) -> String {
        match self {
            Self::Bundled(name) => (*name).to_owned(),
            Self::Written(path) => path.display().to_string(),
            Self::Ui(name) => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../ui/e2e/app-windows/scenarios")
                .join(format!("{name}.jsonl"))
                .display()
                .to_string(),
            Self::Local(name) => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("scenarios")
                .join(format!("{name}.jsonl"))
                .display()
                .to_string(),
        }
    }
}

/// A test's own folder, `<temp>/clauseo-host-<test>-<pid>`, removed when dropped. It is the
/// lawyer's home: the data folder is `root/data` and the new matters' folder `root/Documents/
/// Clauseo`, outside it (`Locations::under_home`); agents get `home/` as their home, and matter
/// folders go in `<temp>/clauseo-host-<test>-<pid>-work`, beside the root: a folder inside the
/// data folder cannot be a matter.
#[derive(Debug)]
pub(crate) struct Dir {
    root: PathBuf,
    data: PathBuf,
    work: PathBuf,
}

impl Dir {
    /// A fresh folder for `test`, with `agents` written into `settings.json`.
    pub(crate) fn new(test: &str, agents: &[(&str, Scenario)]) -> Self {
        // Owner files planted here stay held through the stale rule: no Word runs in a test.
        clauseo_matters::test_support::pretend_word_running_since_epoch();
        let stem = format!("clauseo-host-{test}-{}", std::process::id());
        let root = std::env::temp_dir().join(&stem);
        let work = std::env::temp_dir().join(format!("{stem}-work"));
        write::remove_dir_all(&root).unwrap();
        write::remove_dir_all(&work).unwrap();
        write::create_dir_all(&root.join("home"));
        write::create_dir_all(&root.join("data"));
        write::create_dir_all(&work);
        let data = root.join("data");
        let dir = Self { root, data, work };
        dir.set_agents(agents);
        dir
    }

    /// Writes `settings.json` mapping each id to `fake_agent --scenario <scenario>`. Read at
    /// the next start.
    pub(crate) fn set_agents(&self, agents: &[(&str, Scenario)]) {
        self.set_settings(agents, serde_json::json!({}));
    }

    /// As [`Dir::set_agents`], with the sections of `extra` (an object, such as `{"mcp": ...}`)
    /// beside `agents`.
    pub(crate) fn set_settings(&self, agents: &[(&str, Scenario)], extra: serde_json::Value) {
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
        let mut settings = serde_json::json!({ "agents": map });
        if let (Some(settings), serde_json::Value::Object(extra)) =
            (settings.as_object_mut(), extra)
        {
            settings.extend(extra);
        }
        write::write(
            &self.data().join("settings.json"),
            serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
        );
    }

    /// Writes `<root>/policy.json`, the one policy source the host reads; read at the next start.
    pub(crate) fn set_policy(&self, text: &str) {
        write::write(&self.root.join("policy.json"), text.as_bytes());
    }

    /// Writes `<root>/managed-mcp.json`, where the host looks for Claude Code's enterprise MCP
    /// config (never the system path).
    pub(crate) fn set_managed_mcp(&self) {
        write::write(&self.root.join("managed-mcp.json"), b"{\"mcpServers\": {}}");
    }

    /// The data folder, `root/data`, where the store keeps its database.
    pub(crate) fn data(&self) -> &Path {
        &self.data
    }

    /// The lawyer's home folder: the test's own folder, the data folder inside it.
    pub(crate) fn home(&self) -> &Path {
        &self.root
    }

    /// Makes `work/<name>` holding `files` (relative path, content) and returns its path.
    pub(crate) fn folder(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let folder = self.work_path(name);
        write::create_dir_all(&folder);
        for (path, content) in files {
            write::write(&folder.join(path), content.as_bytes());
        }
        folder
    }

    /// The data folder, as [`Dir::data`].
    pub(crate) fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The folder the matter folders go in, `<root>-work`, beside the data folder.
    pub(crate) fn work(&self) -> &Path {
        &self.work
    }

    /// `name` in the matter folders' place, outside the data folder; not made.
    pub(crate) fn work_path(&self, name: &str) -> PathBuf {
        self.work.join(name)
    }

    /// The config of run `run` (its clock starts `run` hours after [`START_MS`]).
    pub(crate) fn config(&self, run: u16, clock: &ManualClock) -> HostConfig {
        self.config_launching(run, clock, Arc::new(self.launcher()))
    }

    /// The launcher every test uses: no sandbox, with `home/` as the agent's home.
    pub(crate) fn launcher(&self) -> PassthroughLauncher {
        PassthroughLauncher::new(SpawnOptions::new().home(self.root.join("home")))
    }

    /// As [`Dir::config`], launching agents through `launcher`.
    pub(crate) fn config_launching(
        &self,
        run: u16,
        clock: &ManualClock,
        launcher: Arc<dyn Launcher>,
    ) -> HostConfig {
        let locations = Locations::under_home(&self.root);
        let policy = FilePolicy::new(self.root.join("policy.json"), SystemFiles);
        let effective = Effective::load(&locations, &SystemFiles, &[&policy]);
        HostConfig::new(effective, launcher)
            .with_limits_reader(Arc::new(NoReader))
            .with_clock(Arc::new(clock.clone()))
            .with_ids(Box::new(SequentialIds::with_run(run)))
            .with_managed_mcp_path(self.root.join("managed-mcp.json"))
    }

    /// The clock of run `run`.
    pub(crate) fn clock(run: u16) -> ManualClock {
        let start = START_MS + i64::from(run) * 3_600_000;
        ManualClock::new(Timestamp::new(start).unwrap())
    }

    /// The text of `settings.json`.
    pub(crate) fn settings_json(&self) -> String {
        std::fs::read_to_string(self.data().join("settings.json")).unwrap()
    }

    /// Starts run `run` with `policy`, a `policy.json`'s text, as its one policy source.
    pub(crate) fn start_with_policy(&self, run: u16, policy: &str) -> Running {
        self.set_policy(policy);
        self.start(run)
    }

    /// Starts run `run` with every default. A start that finds the data folder's lock still held
    /// is tried again, as [`open_store`] does: the config is rebuilt for each try.
    pub(crate) fn start(&self, run: u16) -> Running {
        const TRIES: u32 = 2_000;
        for _ in 0..TRIES {
            let clock = Self::clock(run);
            let config = self.config_launching(run, &clock, Arc::new(self.launcher()));
            match Host::start(config) {
                Ok((handle, join)) => {
                    return Running {
                        handle,
                        join: Some(join),
                        clock,
                    };
                }
                Err(error) if error.code() == ErrorCode::StoreLocked => {
                    wait(
                        "a store lock",
                        async_io::Timer::after(Duration::from_millis(5)),
                    );
                }
                Err(error) => panic!("Host::start (run {run}): {error}"),
            }
        }
        panic!("Host::start (run {run}): the store stayed locked");
    }

    /// Starts run `run`, letting `adjust` change the config first.
    pub(crate) fn start_with(
        &self,
        run: u16,
        adjust: impl FnOnce(HostConfig) -> HostConfig,
    ) -> Running {
        self.start_launching(run, Arc::new(self.launcher()), adjust)
    }

    /// As [`Dir::start_with`], `adjust` also given the run's clock, for a seam that shares it.
    pub(crate) fn start_clocked(
        &self,
        run: u16,
        adjust: impl FnOnce(HostConfig, &ManualClock) -> HostConfig,
    ) -> Running {
        let clock = Self::clock(run);
        let (handle, join) = Host::start(adjust(self.config(run, &clock), &clock))
            .unwrap_or_else(|e| panic!("Host::start (run {run}): {e}"));
        let join = Some(join);
        Running {
            handle,
            join,
            clock,
        }
    }

    /// As [`Dir::start_with`], launching agents through `launcher`.
    pub(crate) fn start_launching(
        &self,
        run: u16,
        launcher: Arc<dyn Launcher>,
        adjust: impl FnOnce(HostConfig) -> HostConfig,
    ) -> Running {
        let clock = Self::clock(run);
        let (handle, join) = Host::start(adjust(self.config_launching(run, &clock, launcher)))
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
        for folder in [&self.root, &self.work] {
            if let Err(error) = write::remove_dir_all(folder) {
                eprintln!("could not remove {}: {error}", folder.display());
            }
        }
    }
}

/// The harness's limits reader: every read fails at once, and nothing is launched.
pub(crate) struct NoReader;

impl ReadLimits for NoReader {
    fn read(&self, _agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        let failed = ReadFailure::LaunchFailed("the host tests launch no limits reader".to_owned());
        Box::pin(std::future::ready(ReadOutcome::Failed(failed)))
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
            client: None,
            next_date: None,
        });
        match outcome {
            CommandOutcome::MatterCreated { matter } => matter,
            other => panic!("CreateMatter answered {other:?}"),
        }
    }

    /// Registers `folder` as a folder ref, as the desktop's folder picker does.
    pub(crate) fn folder_ref(&self, folder: &Path) -> FolderRefId {
        wait(
            "register_folder",
            self.handle.register_folder(folder.to_owned()),
        )
        .unwrap()
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

/// The shutdowns that move the manual clock while they wait.
#[path = "support_shutdown.rs"]
mod shutdown;
pub(crate) use shutdown::{shutdown_past_cancel, shutdown_stopping};
