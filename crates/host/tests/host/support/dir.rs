//! A test's folder and the host configs and starts it makes: [`Dir`], its folders, its
//! `settings.json` and `policy.json`, and the start on a machine with no Documents folder
//! (issue #365).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_host::{Host, HostConfig, ManualClock, SequentialIds};
use clauseo_process::{Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{ErrorCode, Timestamp};
use clauseo_settings::{Effective, FilePolicy, Locations, SystemFiles};

use super::{NoReader, Running, START_MS, Scenario, wait};
use crate::write;

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
        self.config_at(&Locations::under_home(&self.root), run, clock, launcher)
    }

    /// As [`Dir::config_launching`], with the folders `locations`.
    fn config_at(
        &self,
        locations: &Locations,
        run: u16,
        clock: &ManualClock,
        launcher: Arc<dyn Launcher>,
    ) -> HostConfig {
        let policy = FilePolicy::new(self.root.join("policy.json"), SystemFiles);
        let effective = Effective::load(locations, &SystemFiles, &[&policy]);
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

    /// Starts run `run` as on a machine with no Documents folder (issue #365): the folders of
    /// [`Locations::under_home`] with no default matters folder.
    pub(crate) fn start_without_documents(&self, run: u16) -> Running {
        let clock = Self::clock(run);
        let locations = Locations::under_home(&self.root).without_default_matters_dir();
        let config = self.config_at(&locations, run, &clock, Arc::new(self.launcher()));
        let (handle, join) =
            Host::start(config).unwrap_or_else(|e| panic!("Host::start (run {run}): {e}"));
        Running {
            handle,
            join: Some(join),
            clock,
        }
    }

    /// Starts run `run`, letting `adjust` change the config first.
    pub(crate) fn start_with(
        &self,
        run: u16,
        adjust: impl FnOnce(HostConfig) -> HostConfig,
    ) -> Running {
        self.start_launching(run, Arc::new(self.launcher()), adjust)
    }

    /// As [`Dir::start_launching`], `adjust` also given the run's clock (a seam that shares it).
    pub(crate) fn start_clocked_launching(
        &self,
        run: u16,
        launcher: Arc<dyn Launcher>,
        adjust: impl FnOnce(HostConfig, &ManualClock) -> HostConfig,
    ) -> Running {
        let clock = Self::clock(run);
        let (handle, join) =
            Host::start(adjust(self.config_launching(run, &clock, launcher), &clock))
                .unwrap_or_else(|e| panic!("Host::start (run {run}): {e}"));
        Running {
            handle,
            join: Some(join),
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
