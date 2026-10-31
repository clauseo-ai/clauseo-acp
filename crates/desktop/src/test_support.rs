//! What the bridge's tests share: a scratch data folder, a host started on it, and a channel
//! that collects what the bridge sends. No agent runs: the settings name one (this test binary,
//! an absolute path) that `CreateThread` accepts and no test prompts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
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

/// How long a test waits for one frame before it fails. A bound, not a sleep.
const FRAME_WAIT: Duration = Duration::from_secs(20);

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
        let settings = serde_json::json!({
            "agents": { AGENT: { "command": agent.display().to_string(), "args": args } }
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

/// A channel like a window's, and the frames sent on it.
pub(crate) fn collector() -> (Channel<Frame>, Frames) {
    let (tx, rx) = channel();
    let channel = Channel::new(move |body| {
        let InvokeResponseBody::Json(text) = body else {
            panic!("a frame went as raw bytes");
        };
        let _gone = tx.send(text);
        Ok(())
    });
    (channel, Frames(rx))
}

/// The frames a [`collector`] channel received.
pub(crate) struct Frames(Receiver<String>);

impl Frames {
    /// The next frame's text, as sent.
    pub(crate) fn next_text(&self) -> String {
        self.0
            .recv_timeout(FRAME_WAIT)
            .unwrap_or_else(|err| panic!("no frame: {err}"))
    }

    /// The next frame, an event below the limit, decoded.
    pub(crate) fn next_event(&self) -> HostEvent {
        let text = self.next_text();
        assert!(text.len() < crate::frame::FRAME_LIMIT);
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("not an event ({err}): {text}"))
    }

    /// Frames already received, without waiting.
    pub(crate) fn drain(&self) -> Vec<String> {
        self.0.try_iter().collect()
    }
}

/// The thread's stream.
pub(crate) fn stream(thread: ThreadId) -> StreamId {
    StreamId::Thread(thread)
}
