//! What the bridge's tests share: a scratch data folder, a host started on it, and a channel
//! that collects what the bridge sends. No agent runs: the settings name one (this test binary,
//! an absolute path) that `CreateThread` accepts and no test prompts. The collector sets the
//! limits messages every window gets apart from the rest ([`Messages::next_limits`]).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt::Display;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use clauseo_host::{Host, HostConfig, HostHandle, HostJoin, ManualClock};
use clauseo_limits::ReadLimits;
use clauseo_protocol::{
    AgentId, CommandEnvelope, CommandId, CommandOutcome, HostCommand, HostEvent, HostQuery,
    MatterId, Principal, QueryResult, StreamId, ThreadId, Title,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use tauri::ipc::{Channel, InvokeResponseBody};
use uuid::Uuid;

use crate::bridge::Bridge;
use crate::frame::Frame;

/// The agent id the settings name.
pub(crate) const AGENT: &str = "fake";

/// The agent id that makes an agent Claude, so the host follows its limits.
pub(crate) const CLAUDE: &str = "claude-acp";

/// The folder under a [`Scratch`] root that new matters go in (`matters.default_location`).
pub(crate) const MATTERS_DIR: &str = "matters";

/// How long a test waits for any one step: a host call, the host's start or stop, a channel
/// message, a thread. A bound, not a sleep: past it the test fails naming the step.
///
/// These tests run at the disk's speed: each starts a host, whose first start makes the store and
/// the Scratch matter (16 files created and 32 syncs, counted on macOS). On the Windows runner the
/// disk stalls for minutes at a time (issue #83): in run 41676394591 every test that touched the
/// disk ran 30 to 110 times slower for about 165 s while the rest kept their speed, and seven of
/// these tests, 0.3 to 0.5 s on a good run, passed 30 s. So nextest gives them 160 s
/// (`.config/nextest.toml`), and one step may take 120 s: longer than any step takes in a
/// stall (each is well under 0.1 s on a good run), shorter than the test's bound, so a hung step
/// fails with its name and not as a bare timeout.
pub(crate) const STEP_WAIT: Duration = Duration::from_secs(120);

/// Wakes the thread that polls a future.
struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `future` on this thread for at most [`STEP_WAIT`]: its output, or `None` past that.
/// A timer thread of its own marks the deadline (the crate reads no clock), and ends as soon as
/// the future does.
fn try_within<T>(future: impl Future<Output = T>) -> Option<T> {
    let waiting = std::thread::current();
    let expired = Arc::new(AtomicBool::new(false));
    let (_finished, finish) = channel::<()>();
    {
        let (expired, waiting) = (Arc::clone(&expired), waiting.clone());
        std::thread::spawn(move || {
            if finish.recv_timeout(STEP_WAIT) == Err(RecvTimeoutError::Timeout) {
                expired.store(true, Ordering::SeqCst);
                waiting.unpark();
            }
        });
    }
    let waker = Waker::from(Arc::new(Unpark(waiting)));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return Some(output);
        }
        if expired.load(Ordering::SeqCst) {
            return None;
        }
        std::thread::park();
    }
}

/// Runs `future` on this thread; past [`STEP_WAIT`] the test fails naming `step`.
#[track_caller]
pub(crate) fn within<T>(step: impl Display, future: impl Future<Output = T>) -> T {
    match try_within(future) {
        Some(output) => output,
        None => panic!("{step}: no answer in {} s", STEP_WAIT.as_secs()),
    }
}

/// `futures::executor::block_on` with a deadline: past [`STEP_WAIT`] the test fails naming
/// the line that waited.
#[track_caller]
pub(crate) fn block_on<T>(future: impl Future<Output = T>) -> T {
    let caller = std::panic::Location::caller();
    within(format_args!("the wait at {caller}"), future)
}

/// Runs `work` on a thread of its own and answers its result; past [`STEP_WAIT`] the test
/// fails naming `step`. For a call that blocks, which [`within`] cannot bound.
#[track_caller]
fn on_thread<T: Send + 'static>(step: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, result) = channel();
    std::thread::spawn(move || {
        let _gone = done.send(work());
    });
    match result.recv_timeout(STEP_WAIT) {
        Ok(output) => output,
        Err(err) => panic!("{step}: {err} in {} s", STEP_WAIT.as_secs()),
    }
}

/// `<temp>/clauseo-desktop-<test>-<pid>`, removed when dropped. The data folder is the root
/// `data` inside it (`Locations::under_home`), so nothing touches the real app data folder.
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// A fresh folder for `test` with `settings.json` naming [`AGENT`] and the folders in
    /// `dirs` made under it. The agent is never started.
    pub(crate) fn new(test: &str, dirs: &[&str]) -> Self {
        let agent = std::env::current_exe().unwrap();
        Self::with_agent(test, dirs, Some(&agent), &[], false)
    }

    /// A fresh folder for `test` with the folders in `dirs` made under it and nothing else: no
    /// settings, so no host starts on it. A place outside another scratch's data folder.
    pub(crate) fn folder(test: &str, dirs: &[&str]) -> Self {
        Self::with_agent(test, dirs, None, &[], false)
    }

    /// [`Scratch::new`] whose settings also name [`CLAUDE`], so the host has limits to read.
    /// Start it with [`Scratch::start_reading`]: the default reader would launch a process.
    pub(crate) fn with_limits(test: &str) -> Self {
        let agent = std::env::current_exe().unwrap();
        Self::with_agent(test, &[], Some(&agent), &[], true)
    }

    /// [`Scratch::new`] with [`AGENT`] playing the bundled `fake_agent` scenario `scenario`.
    /// `fake_agent` is built only in a workspace run:
    /// `cargo nextest run --workspace -E 'package(clauseo-desktop)'`, or
    /// `cargo build -p clauseo-fake_agent` first.
    pub(crate) fn with_fake_agent(test: &str, scenario: &str) -> Self {
        let bin = clauseo_fake_agent::bin_path()
            .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
        Self::with_agent(test, &[], Some(&bin), &["--scenario", scenario], false)
    }

    /// A fresh folder whose settings run `agent` with `args` for [`AGENT`], and for [`CLAUDE`]
    /// too with `claude`; with no `agent`, no settings.
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    fn with_agent(
        test: &str,
        dirs: &[&str],
        agent: Option<&Path>,
        args: &[&str],
        claude: bool,
    ) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-desktop-{test}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        for dir in dirs {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let Some(agent) = agent else {
            return Self { root };
        };
        // New matters, the Scratch matter included, go in the scratch folder on every OS, set
        // here and not only through `Locations::under_home`: never the real Documents folder.
        let entry = serde_json::json!({ "command": agent.display().to_string(), "args": args });
        let mut agents = serde_json::json!({ AGENT: entry });
        if claude {
            agents[CLAUDE] = entry;
        }
        let settings = serde_json::json!({
            "agents": agents,
            "matters": { "default_location": root.join(MATTERS_DIR).display().to_string() }
        });
        // The settings file is in the data folder, `root/data` (`Locations::under_home`).
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("data").join("settings.json"),
            settings.to_string(),
        )
        .unwrap();
        Self { root }
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    /// Makes the folder `rel` under the scratch folder, with its parents, and answers its path.
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn dir(&self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// Writes `bytes` to the file `rel` under the scratch folder (its folder must exist), and
    /// answers its path.
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn file(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Marks the file `rel` under the scratch folder as a program (mode 0755).
    #[cfg(all(unix, not(target_os = "macos")))]
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn executable(&self, rel: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        let path = self.root.join(rel);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Makes `rel` under the scratch folder a symbolic link to `target`, and answers its path.
    #[cfg(unix)]
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    pub(crate) fn link(&self, target: &Path, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::os::unix::fs::symlink(target, &path).unwrap();
        path
    }

    /// The launcher the app's own start-up path (`host_config`) gives a host on the folder.
    pub(crate) fn app_launcher(&self) -> Arc<dyn clauseo_process::Launcher> {
        let effective = Effective::load(&Locations::under_home(&self.root), &SystemFiles, &[]);
        crate::host_config(effective).launcher()
    }

    /// The config of a host on the folder, refusing `refused` as a matter.
    pub(crate) fn config(&self, refused: Vec<PathBuf>) -> HostConfig {
        let effective = Effective::load(&Locations::under_home(&self.root), &SystemFiles, &[]);
        let launcher = crate::host_launcher(None);
        HostConfig::new(effective, Arc::new(launcher)).with_refused_folders(refused)
    }

    /// Starts a host on the folder, refusing `refused` as a matter.
    #[track_caller]
    pub(crate) fn start(&self, refused: Vec<PathBuf>) -> Running {
        Self::start_config(self.config(refused))
    }

    /// Starts a host on the folder that reads limits through `reader` on `clock`.
    #[track_caller]
    pub(crate) fn start_reading(&self, reader: Arc<dyn ReadLimits>, clock: ManualClock) -> Running {
        let config = self.config(Vec::new());
        Self::start_config(
            config
                .with_limits_reader(reader)
                .with_clock(Arc::new(clock)),
        )
    }

    /// Starts a host with `config`.
    #[track_caller]
    fn start_config(config: HostConfig) -> Running {
        let (host, join) = on_thread("the host's start", move || Host::start(config)).unwrap();
        Running {
            host,
            join: Some(join),
        }
    }
}

impl Drop for Scratch {
    #[expect(
        clippy::disallowed_methods,
        reason = "test clean-up removes its scratch folder; the desktop crate has no write module"
    )]
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.root);
    }
}

/// A running host, shut down and joined when dropped.
pub(crate) struct Running {
    pub(crate) host: HostHandle,
    join: Option<HostJoin>,
}

impl Drop for Running {
    /// Each wait is bounded by [`STEP_WAIT`]. Past it the test fails, unless it is already
    /// failing: a panic in a drop while unwinding would abort and hide the first panic's message.
    fn drop(&mut self) {
        let stopped = try_within(self.host.shutdown()).is_some();
        let joined = stopped
            && self.join.take().is_none_or(|join| {
                let (done, result) = channel();
                std::thread::spawn(move || {
                    let _gone = done.send(join.join());
                });
                result.recv_timeout(STEP_WAIT).is_ok()
            });
        if !joined {
            let step = if stopped { "joining" } else { "shutting down" };
            let message = format!("{step} the host: no answer in {} s", STEP_WAIT.as_secs());
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

impl Running {
    /// A bridge on this host.
    pub(crate) fn bridge(&self) -> Bridge {
        Bridge::new(crate::host_start::HostCell::ready(self.host.clone()))
    }

    /// The Scratch matter, which a first start creates.
    pub(crate) fn scratch(&self) -> MatterId {
        let list = within(
            "ListMatters",
            self.host
                .query(Principal::LocalDesktop, HostQuery::ListMatters),
        );
        let Ok(QueryResult::Matters(matters)) = list else {
            panic!("no matter list: {list:?}");
        };
        matters.first().unwrap().matter
    }

    /// Runs `command` straight on the host.
    pub(crate) fn run(&self, command: HostCommand) -> CommandOutcome {
        let step = format!("{command:?} on the host");
        within(step, self.host.dispatch(envelope(command))).unwrap()
    }

    /// A new thread in the Scratch matter.
    pub(crate) fn thread(&self) -> ThreadId {
        let outcome = self.run(HostCommand::CreateThread {
            matter: self.scratch(),
            agent: AgentId::new(AGENT).unwrap(),
        });
        let CommandOutcome::ThreadCreated { thread } = outcome else {
            panic!("not created: {outcome:?}");
        };
        thread
    }

    /// Renames `thread`, which changes its header: one patch on its stream.
    pub(crate) fn rename(&self, thread: ThreadId, title: &str) {
        self.run(HostCommand::RenameThread {
            thread,
            title: Title::new(title).unwrap(),
            placeholder: false,
        });
    }
}

/// `command` with a fresh id.
pub(crate) fn envelope(command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: CommandId::new(Uuid::now_v7()).unwrap(),
        command,
    }
}

/// Opens the window `label`'s channel on `bridge` and waits for the limits view the host sends
/// it first, which stays unacknowledged: the window has been sent one message.
pub(crate) fn open(bridge: &Bridge, label: &str) -> Messages {
    let (channel, messages) = collector();
    within(
        format_args!("open_stream for {label}"),
        bridge.open_stream(label, channel),
    )
    .unwrap();
    let first = messages.next_limits();
    assert!(
        matches!(first, HostEvent::LimitsViewChanged(_)),
        "not the view first: {first:?}"
    );
    messages
}

/// Whether `text` is a limits event, whose wire keys are not `fragment`, `stream_failed` or
/// `timed`.
fn is_limits(text: &str) -> bool {
    text.starts_with(r#"{"limits_view_changed":"#)
        || text.starts_with(r#"{"account_limits_changed":"#)
}

/// A channel like a window's, and the messages sent on it. In the e2e build it reads a `timed`
/// message as the page does: it keeps the event inside and counts the message as timed.
pub(crate) fn collector() -> (Channel<Frame>, Messages) {
    let (tx, rx) = channel();
    let timed = Arc::new(Vec::new().into());
    let seen = Arc::clone(&timed);
    let channel = Channel::new(move |body| {
        let InvokeResponseBody::Json(text) = body else {
            panic!("a channel message went as raw bytes");
        };
        let text = untimed(text, &seen);
        let _gone = tx.send(text);
        Ok(())
    });
    (
        channel,
        Messages {
            rx,
            timed,
            held: RefCell::new(VecDeque::new()),
            limits_received: Cell::new(0),
            limits_bytes: Cell::new(0),
        },
    )
}

/// `text` itself, or the event of a `timed` message, whose time and length go in `seen`.
fn untimed(text: String, seen: &Mutex<Vec<(f64, usize)>>) -> String {
    let Some(rest) = text.strip_prefix(r#"{"timed":{"received_ms":"#) else {
        return text;
    };
    let (time, rest) = rest.split_once(r#","event":"#).unwrap();
    let event = rest.strip_suffix("}}").unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed["timed"]["event"],
        serde_json::from_str::<serde_json::Value>(event).unwrap()
    );
    let received_ms: f64 = time.parse().unwrap();
    assert_eq!(text.len(), wrapper_len(received_ms) + event.len());
    crate::bridge::lock(seen).push((received_ms, text.len()));
    event.to_owned()
}

/// The bytes a `timed` wrapper with `received_ms` adds around its event.
fn wrapper_len(received_ms: f64) -> usize {
    format!(r#"{{"timed":{{"received_ms":{received_ms},"event":}}}}"#).len()
}

/// The messages a [`collector`] channel received.
pub(crate) struct Messages {
    rx: Receiver<String>,
    /// The receive time and length of each `timed` message, in order.
    timed: Arc<Mutex<Vec<(f64, usize)>>>,
    /// Messages received and not yet returned, in order.
    held: RefCell<VecDeque<String>>,
    /// The limits messages received so far.
    limits_received: Cell<u32>,
    /// Their bytes.
    limits_bytes: Cell<usize>,
}

impl Messages {
    /// The first held message `wanted` takes, else the next received one, holding the rest.
    fn next_where(&self, wanted: impl Fn(&str) -> bool) -> String {
        let mut held = self.held.borrow_mut();
        if let Some(at) = held.iter().position(|text| wanted(text)) {
            return held.remove(at).unwrap();
        }
        loop {
            let text = self.rx.recv_timeout(STEP_WAIT).unwrap_or_else(|err| {
                panic!("no channel message in {} s: {err}", STEP_WAIT.as_secs())
            });
            self.count(&text);
            if wanted(&text) {
                return text;
            }
            held.push_back(text);
        }
    }

    /// Counts `text`, just received, and its bytes, when it is a limits message.
    fn count(&self, text: &str) {
        if is_limits(text) {
            self.limits_received.set(self.limits_received.get() + 1);
            self.limits_bytes.set(self.limits_bytes.get() + text.len());
        }
    }

    /// The next message's text, as sent, that is not a limits message.
    pub(crate) fn next_text(&self) -> String {
        self.next_where(|text| !is_limits(text))
    }

    /// The next limits message, decoded.
    pub(crate) fn next_limits(&self) -> HostEvent {
        let text = self.next_where(is_limits);
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("not an event ({err}): {text}"))
    }

    /// How many limits messages were received so far, returned or not: for acks, which count
    /// every message.
    pub(crate) fn limits_received(&self) -> u32 {
        self.limits_received.get()
    }

    /// The bytes of the limits messages received so far, returned or not: the window counts them
    /// toward its cap with every other message.
    pub(crate) fn limits_bytes(&self) -> usize {
        self.limits_bytes.get()
    }

    /// The next message, an event below the limit, decoded.
    pub(crate) fn next_event(&self) -> HostEvent {
        let text = self.next_text();
        assert!(text.len() < crate::frame::FRAME_LIMIT);
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("not an event ({err}): {text}"))
    }

    /// Messages already received, without waiting, but the limits messages.
    pub(crate) fn drain(&self) -> Vec<String> {
        let mut held = self.held.borrow_mut();
        for text in self.rx.try_iter() {
            self.count(&text);
            held.push_back(text);
        }
        let (limits, rest): (VecDeque<String>, VecDeque<String>) =
            held.drain(..).partition(|text| is_limits(text));
        *held = limits;
        rest.into()
    }

    /// The receive time and length of each `timed` message so far.
    pub(crate) fn timed(&self) -> Vec<(f64, usize)> {
        crate::bridge::lock(&self.timed).clone()
    }

    /// The bytes `timed` wrappers added to the messages so far: what the bridge counted beyond
    /// the texts [`next_text`](Self::next_text) returned. 0 outside the e2e build.
    pub(crate) fn timed_overhead(&self) -> usize {
        let timed = crate::bridge::lock(&self.timed);
        timed.iter().map(|&(time, _)| wrapper_len(time)).sum()
    }
}

/// The thread's stream.
pub(crate) fn stream(thread: ThreadId) -> StreamId {
    StreamId::Thread(thread)
}

/// A host started on a [`Scratch`] makes its Scratch matter inside the scratch folder, on every
/// OS: never in the real Documents folder.
#[test]
fn the_scratch_matter_stays_in_the_scratch_folder() {
    let scratch = Scratch::new("scratch-matter", &[]);
    let host = scratch.start(Vec::new());
    let _matter = host.scratch();
    let made = scratch.path().join(MATTERS_DIR).join("Scratch");
    assert!(made.is_dir(), "{}", made.display());
}
