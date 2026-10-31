//! What the bridge's tests share: a scratch data folder, a host started on it, and a channel
//! that collects what the bridge sends. No agent runs: the settings name one (this test binary,
//! an absolute path) that `CreateThread` accepts and no test prompts.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{Host, HostConfig, HostHandle, HostJoin};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
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

/// The folder under a [`Scratch`] root that new matters go in (`matters.default_location`).
pub(crate) const MATTERS_DIR: &str = "matters";

/// How long a test waits for one channel message before it fails. A bound, not a sleep.
const MESSAGE_WAIT: Duration = Duration::from_secs(20);

/// `<temp>/clauseo-desktop-<test>-<pid>`, removed when dropped. The data folder is the root
/// itself (`Locations::under_root`), so nothing touches the real app data folder.
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// A fresh folder for `test` with `settings.json` naming [`AGENT`] and the folders in
    /// `dirs` made under it. The agent is never started.
    pub(crate) fn new(test: &str, dirs: &[&str]) -> Self {
        let agent = std::env::current_exe().unwrap();
        Self::with_agent(test, dirs, &agent, &[])
    }

    /// [`Scratch::new`] with [`AGENT`] playing the bundled `fake_agent` scenario `scenario`.
    /// `fake_agent` is built only in a workspace run:
    /// `cargo nextest run --workspace -E 'package(clauseo-desktop)'`, or
    /// `cargo build -p clauseo-fake_agent` first.
    pub(crate) fn with_fake_agent(test: &str, scenario: &str) -> Self {
        let bin = clauseo_fake_agent::bin_path()
            .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
        Self::with_agent(test, &[], &bin, &["--scenario", scenario])
    }

    /// A fresh folder whose settings run `agent` with `args` for [`AGENT`].
    #[expect(
        clippy::disallowed_methods,
        reason = "test set-up writes its scratch folder; the desktop crate has no write module"
    )]
    fn with_agent(test: &str, dirs: &[&str], agent: &Path, args: &[&str]) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-desktop-{test}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        for dir in dirs {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // New matters, the Scratch matter included, go in the scratch folder on every OS, set
        // here and not only through `Locations::under_root`: never the real Documents folder.
        let settings = serde_json::json!({
            "agents": { AGENT: { "command": agent.display().to_string(), "args": args } },
            "matters": { "default_location": root.join(MATTERS_DIR).display().to_string() }
        });
        std::fs::write(root.join("settings.json"), settings.to_string()).unwrap();
        Self { root }
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    /// Starts a host on the folder, refusing `refused` as a matter.
    pub(crate) fn start(&self, refused: Vec<PathBuf>) -> Running {
        let effective = Effective::load(&Locations::under_root(&self.root), &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new());
        let config = HostConfig::new(effective, Arc::new(launcher)).with_refused_folders(refused);
        let (host, join) = Host::start(config).unwrap();
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
    fn drop(&mut self) {
        let _stopped = futures::executor::block_on(self.host.shutdown());
        if let Some(join) = self.join.take() {
            let _joined = join.join();
        }
    }
}

impl Running {
    /// A bridge on this host.
    pub(crate) fn bridge(&self) -> Bridge {
        Bridge::new(self.host.clone())
    }

    /// The Scratch matter, which a first start creates.
    pub(crate) fn scratch(&self) -> MatterId {
        let list = futures::executor::block_on(
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
        futures::executor::block_on(self.host.dispatch(envelope(command))).unwrap()
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
    (channel, Messages { rx, timed })
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
}

impl Messages {
    /// The next message's text, as sent.
    pub(crate) fn next_text(&self) -> String {
        self.rx
            .recv_timeout(MESSAGE_WAIT)
            .unwrap_or_else(|err| panic!("no channel message: {err}"))
    }

    /// The next message, an event below the limit, decoded.
    pub(crate) fn next_event(&self) -> HostEvent {
        let text = self.next_text();
        assert!(text.len() < crate::frame::FRAME_LIMIT);
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("not an event ({err}): {text}"))
    }

    /// Messages already received, without waiting.
    pub(crate) fn drain(&self) -> Vec<String> {
        self.rx.try_iter().collect()
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
