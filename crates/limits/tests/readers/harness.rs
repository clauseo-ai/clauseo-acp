//! The test launcher, the shared manual timer, and each test's fake install and data folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_limits::ProcessReaders;
use clauseo_process::{
    Error, LaunchSpec, Launched, Launcher, ManualTimer, PassthroughLauncher, SpawnOptions, Timer,
};
use clauseo_settings::{Effective, Locations, SystemFiles};

use crate::write;

/// The adapter's script under the fake install, as `args[0]`.
pub(crate) const SCRIPT: &str = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
/// The Claude Agent SDK's `package.json` under the fake install.
const SDK: &str = "node_modules/@anthropic-ai/claude-agent-sdk/package.json";
/// What the fake SDK's `package.json` holds: the pinned SDK's entry, `sdk.mjs`.
const SDK_PACKAGE: &str = r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"0.3.293",
"type":"module","main":"sdk.mjs","exports":{".":{"types":"./sdk.d.ts","default":"./sdk.mjs"}}}"#;
/// The fake SDK's entry file under the fake install.
const SDK_ENTRY: &str = "node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs";
/// The helper's file name in the install folder.
pub(crate) const HELPER: &str = "clauseo-claude-usage.mjs";
/// A fake 20-character key the test sets in settings env; `claude-crash` echoes it.
pub(crate) const FAKE_KEY: &str = "fakeAnthropicKey0123";
/// A fake 20-character key the test sets in Codex's settings env; `codex-crash` echoes it.
pub(crate) const FAKE_OPENAI_KEY: &str = "fakeOpenAiKey0123456";
/// codex-acp's script under the fake install.
pub(crate) const CODEX_SCRIPT: &str = "node_modules/@agentclientprotocol/codex-acp/dist/index.js";
/// Where the Codex CLI's script sits under a `node_modules` folder.
pub(crate) const CODEX_JS: &str = "@openai/codex/bin/codex.js";
/// An inherited variable the settings env overrides in another case; cargo and nextest set it
/// for every test run.
pub(crate) const INHERITED: &str = "CARGO_MANIFEST_DIR";

/// `rel` (`/`-separated) under `base`, joined one component at a time, so it is spelled with
/// the OS's own separator and compares equal to a path the reader built.
pub(crate) fn under(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// A manual timer the reader can clone.
#[derive(Clone, Default)]
pub(crate) struct SharedTimer(pub(crate) Arc<ManualTimer>);

impl Timer for SharedTimer {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        self.0.after(duration)
    }
}

/// What the test launcher starts instead of the helper.
pub(crate) enum Play {
    /// `fake_agent --scenario tests/scenarios/<name>.jsonl`.
    Scenario(&'static str),
    /// `fake_agent --hold`.
    Hold,
}

/// Starts `fake_agent` in place of the spec's program and arguments, with the spec's env and
/// working folder, and records every spec it is given.
pub(crate) struct TestLauncher {
    play: Play,
    inner: PassthroughLauncher,
    specs: Mutex<Vec<LaunchSpec>>,
    /// A copy of each started agent's stdout read end (Unix), to see the tree end.
    #[cfg(unix)]
    stdouts: Mutex<Vec<std::os::fd::OwnedFd>>,
}

impl TestLauncher {
    pub(crate) fn new(play: Play) -> Arc<TestLauncher> {
        Arc::new(TestLauncher {
            play,
            inner: PassthroughLauncher::new(SpawnOptions::new()),
            specs: Mutex::new(Vec::new()),
            #[cfg(unix)]
            stdouts: Mutex::new(Vec::new()),
        })
    }

    /// How many specs the reader asked to launch. Compare this, not the specs: a failed compare
    /// prints them, and their env holds the machine's.
    pub(crate) fn launches(&self) -> usize {
        self.specs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Every spec the reader asked to launch. Never `Debug`-print one: its env holds the
    /// inherited environment, secrets included.
    pub(crate) fn specs(&self) -> Vec<LaunchSpec> {
        self.specs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits until every process holding the last agent's stdout has ended: its read end then
    /// reads end of file. A survivor blocks until nextest's slow-timeout ends the test. The wait
    /// is Unix only (it watches a copy of the pipe's fd); elsewhere it checks only that an agent
    /// was started.
    pub(crate) fn wait_stdout_closed(&self) {
        #[cfg(not(unix))]
        assert!(self.launches() > 0, "an agent was started");
        #[cfg(unix)]
        {
            use futures::AsyncReadExt as _;
            let fd = self
                .stdouts
                .lock()
                .unwrap()
                .pop()
                .expect("an agent was started");
            let mut stdout = async_io::Async::new(std::fs::File::from(fd)).expect("watch stdout");
            let mut buf = [0; 4096];
            futures::executor::block_on(async {
                while stdout.read(&mut buf).await.unwrap() > 0 {}
            });
        }
    }
}

impl Launcher for TestLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        self.specs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(spec.clone());
        let bin = clauseo_fake_agent::bin_path()
            .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
        let args = match self.play {
            Play::Scenario(name) => vec![
                OsString::from("--scenario"),
                under(Path::new(env!("CARGO_MANIFEST_DIR")), "tests/scenarios")
                    .join(format!("{name}.jsonl"))
                    .into_os_string(),
            ],
            Play::Hold => vec![OsString::from("--hold")],
        };
        let swapped = LaunchSpec::new(bin, args, spec.env().clone(), spec.cwd().to_owned())?;
        let launched = self.inner.launch(&swapped)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsFd as _;
            let copy = launched
                .stdout
                .as_fd()
                .try_clone_to_owned()
                .expect("copy stdout");
            self.stdouts.lock().unwrap().push(copy);
        }
        Ok(launched)
    }
}

/// Numbers the folders of one process, so tests that share it (`cargo test`) get one each.
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A test's folder: `install/` with the adapter's script, and `data/` with `settings.json`.
/// Removed on drop.
pub(crate) struct Setup {
    pub(crate) root: PathBuf,
}

impl Setup {
    /// A fresh folder with a fake install (the adapter's script, the SDK's `package.json` and its
    /// entry) and an empty data folder.
    pub(crate) fn new() -> Setup {
        let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("clauseo-limits-readers-{}-{n}", std::process::id()));
        let setup = Setup { root };
        write::create_dir_all(&setup.data()).unwrap();
        Setup::file(&setup.script(), "");
        Setup::file(&under(&setup.install(), SDK), SDK_PACKAGE);
        Setup::file(&under(&setup.install(), SDK_ENTRY), "");
        setup
    }

    pub(crate) fn install(&self) -> PathBuf {
        self.root.join("install")
    }

    pub(crate) fn script(&self) -> PathBuf {
        under(&self.install(), SCRIPT)
    }

    pub(crate) fn helper(&self) -> PathBuf {
        self.install().join(HELPER)
    }

    pub(crate) fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    pub(crate) fn run_dir(&self) -> PathBuf {
        self.data().join("limits").join("run")
    }

    /// Writes `contents` to `path`, making its folders.
    pub(crate) fn file(path: &Path, contents: &str) {
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write(path, contents).unwrap();
    }

    /// The settings `command`: an absolute path the test launcher never runs.
    pub(crate) fn command() -> String {
        if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        }
    }

    /// Writes `settings.json` with `agents.claude-acp` running `script`.
    pub(crate) fn settings(&self, script: &Path) {
        self.write_settings(Some(script), None);
    }

    /// Writes `settings.json` with `agents.codex-acp` running `script`.
    pub(crate) fn codex_settings(&self, script: &Path) {
        self.write_settings(None, Some(script));
    }

    /// Writes `settings.json` with the agents whose scripts are given.
    fn write_settings(&self, claude: Option<&Path>, codex: Option<&Path>) {
        let mut agents = serde_json::Map::new();
        if let Some(script) = claude {
            let entry = serde_json::json!({
                "command": Setup::command(),
                "args": [script],
                "env": {
                    "ANTHROPIC_API_KEY": FAKE_KEY,
                    INHERITED.to_ascii_lowercase(): "from-settings",
                },
            });
            agents.insert("claude-acp".to_owned(), entry);
        }
        if let Some(script) = codex {
            let entry = serde_json::json!({
                "command": Setup::command(),
                "args": [script],
                "env": {"OPENAI_API_KEY": FAKE_OPENAI_KEY, "codex_home": "/from/settings"},
            });
            agents.insert("codex-acp".to_owned(), entry);
        }
        let settings = serde_json::json!({ "agents": agents });
        write::write(&self.data().join("settings.json"), settings.to_string()).unwrap();
    }

    /// codex-acp's script under the fake install.
    pub(crate) fn codex_script(&self) -> PathBuf {
        under(&self.install(), CODEX_SCRIPT)
    }

    /// The Codex home given to `ProcessReaders::new`; never made.
    pub(crate) fn codex_home(&self) -> PathBuf {
        self.data().join("codex-home")
    }

    /// The settings as `Effective::load` reads them from the data folder.
    pub(crate) fn effective(&self) -> Effective {
        Effective::load(&Locations::under_root(&self.data()), &SystemFiles, &[])
    }

    /// Readers over `launcher` and `timer`, from the settings in the data folder.
    pub(crate) fn readers(
        &self,
        launcher: &Arc<TestLauncher>,
        timer: &SharedTimer,
    ) -> ProcessReaders<SharedTimer> {
        let launcher: Arc<dyn Launcher> = Arc::<TestLauncher>::clone(launcher);
        ProcessReaders::new(
            &self.effective(),
            self.codex_home(),
            launcher,
            timer.clone(),
        )
    }

    /// The names in `dir`, sorted; empty when it is missing.
    pub(crate) fn names(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The env a spec should carry, by the case rule: inherited minus what settings names in
    /// any case, then the settings variables.
    pub(crate) fn expected_env() -> BTreeMap<OsString, OsString> {
        let mut env: BTreeMap<OsString, OsString> = std::env::vars_os()
            .filter(|(name, _)| {
                !name.eq_ignore_ascii_case("ANTHROPIC_API_KEY")
                    && !name.eq_ignore_ascii_case(INHERITED)
            })
            .collect();
        env.insert("ANTHROPIC_API_KEY".into(), FAKE_KEY.into());
        env.insert(
            INHERITED.to_ascii_lowercase().into(),
            "from-settings".into(),
        );
        env
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.root));
    }
}
