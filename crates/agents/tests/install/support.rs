//! The fixture every install test starts from: two agents made of tiny tarballs, served by the
//! local test registry under the host name `registry.test`, the real Node and npm found on
//! `PATH`, a shared `ManualTimer` that records every wait armed, and a clock whose monotonic
//! reading moves 250 ms per call (so every progress tick is due).

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use clauseo_agents::testing::sha512_integrity;
use clauseo_agents::testing::{Lookups, TempDir, TestRegistry, npm_tarball, resolve_all_to};
use clauseo_agents::{
    AgentEntry, AgentSizes, Agents, AgentsConfig, AgentsEvent, Clock, InstallState, NodeArchive,
    NodeEntry, NodeSearch, Platform, Snapshot,
};
use clauseo_process::{
    LaunchSpec, Launched, Launcher, ManualTimer, PassthroughLauncher, SpawnOptions, Timer,
};
use clauseo_protocol::AgentId;

use crate::write;

/// The fixture's registry base.
pub(crate) const BASE: &str = "http://registry.test/";
/// Node's version in the fixture snapshot, and its download folder.
pub(crate) const NODE_VERSION: &str = "24.22.0";
pub(crate) const NODE_BASE: &str = "http://nodejs.test/dist/v24.22.0/";

/// The two agents, their root packages and their other packages.
pub(crate) const A_PACKAGES: [&str; 3] = ["adapter-a", "dep-a", "peer-a"];
pub(crate) const B_PACKAGES: [&str; 2] = ["adapter-b", "dep-b"];

pub(crate) fn agent_a() -> AgentId {
    AgentId::new("agent-a").unwrap()
}

pub(crate) fn agent_b() -> AgentId {
    AgentId::new("agent-b").unwrap()
}

/// The registry path of package `name`'s tarball.
pub(crate) fn tarball_path(name: &str) -> String {
    format!("/{name}/-/{name}-1.0.0.tgz")
}

/// This machine's platform; the tests run only where Clauseo installs.
pub(crate) fn platform() -> Platform {
    Platform::current().expect("a platform Clauseo installs on")
}

/// The real Node's program and npm's `npm-cli.js` beside it, from `PATH`.
pub(crate) fn real_node() -> (PathBuf, PathBuf) {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let program = write::on_path(name)
        .and_then(|program| dunce::canonicalize(program).ok())
        .expect(
            "these tests need Node 22 or later with npm on PATH (CI's build jobs have Node 24): \
             no node found",
        );
    let dir = program.parent().unwrap();
    let cli = if cfg!(windows) {
        dir.join("node_modules/npm/bin/npm-cli.js")
    } else {
        dir.join("../lib/node_modules/npm/bin/npm-cli.js")
    };
    assert!(
        cli.is_file(),
        "these tests need npm beside the node on PATH; {} has none",
        program.display()
    );
    (program, dunce::canonicalize(cli).unwrap())
}

/// Every fixture tarball, by package name.
pub(crate) fn tarballs() -> BTreeMap<&'static str, Vec<u8>> {
    let script: &[u8] = b"process.stdout.write('fixture adapter\\n');\n";
    let mut out = BTreeMap::new();
    for name in A_PACKAGES.into_iter().chain(B_PACKAGES) {
        let files: Vec<(&str, &[u8])> = if name.starts_with("adapter-") {
            vec![("dist/index.js", script)]
        } else {
            vec![("index.js", b"module.exports = 1;\n")]
        };
        out.insert(name, npm_tarball(name, "1.0.0", &files).unwrap());
    }
    out
}

/// A valid lockfile-v3 pins pair: `adapter-a` with a dependency and a peer, `adapter-b` with
/// its own dependency.
pub(crate) fn pins(base: &str, tarballs: &BTreeMap<&str, Vec<u8>>) -> (String, String) {
    let deps = serde_json::json!({ "adapter-a": "1.0.0", "adapter-b": "1.0.0" });
    let package_json = serde_json::json!({
        "name": "fixture", "version": "1.0.0", "private": true, "dependencies": deps,
    });
    let entry = |name: &str| {
        serde_json::json!({
            "version": "1.0.0",
            "resolved": format!("{base}{name}/-/{name}-1.0.0.tgz"),
            "integrity": sha512_integrity(&tarballs[name]),
            "license": "MIT",
        })
    };
    let mut packages = serde_json::Map::new();
    packages.insert(
        String::new(),
        serde_json::json!({ "name": "fixture", "version": "1.0.0", "dependencies": deps }),
    );
    for name in A_PACKAGES.into_iter().chain(B_PACKAGES) {
        let mut value = entry(name);
        match name {
            "adapter-a" => {
                value["dependencies"] = serde_json::json!({ "dep-a": "1.0.0" });
                value["peerDependencies"] = serde_json::json!({ "peer-a": "^1.0.0" });
            }
            "adapter-b" => value["dependencies"] = serde_json::json!({ "dep-b": "1.0.0" }),
            "peer-a" => value["peer"] = serde_json::json!(true),
            _ => {}
        }
        packages.insert(format!("node_modules/{name}"), value);
    }
    let lock = serde_json::json!({
        "name": "fixture", "version": "1.0.0", "lockfileVersion": 3, "requires": true,
        "packages": packages,
    });
    (
        serde_json::to_string_pretty(&package_json).unwrap(),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
}

/// Node's entry with one archive for this platform: `bytes` when given (its real size and
/// SHA-256), else a stand-in that is never fetched.
pub(crate) fn node_entry(bytes: Option<&[u8]>) -> NodeEntry {
    let platform = platform();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let name = format!("node-v{NODE_VERSION}-{}.{ext}", platform.node_name());
    let (size, sha) = match bytes {
        Some(bytes) => {
            use sha2::Digest as _;
            let digest = sha2::Sha256::digest(bytes);
            let hex = to_hex(&digest);
            (u64::try_from(bytes.len()).unwrap(), hex)
        }
        None => (1000, "0".repeat(64)),
    };
    let archive = NodeArchive::new(platform.os, platform.arch, &name, size, 4000, &sha).unwrap();
    NodeEntry::new(NODE_VERSION, NODE_BASE, vec![archive])
}

/// The registry path of Node's archive for this platform. Only the Unix tests download Node.
#[cfg(unix)]
pub(crate) fn node_path(entry: &NodeEntry) -> String {
    let archive = entry.archive(platform()).unwrap();
    let url = entry.url(archive);
    url.strip_prefix("http://nodejs.test").unwrap().to_owned()
}

/// The fixture snapshot: both agents, sized from the tarballs, each unpacking to `unpacked`.
pub(crate) fn snapshot(base: &str, node: NodeEntry, unpacked: u64) -> Snapshot {
    let tarballs = tarballs();
    let (package_json, lock) = pins(base, &tarballs);
    let platform = platform();
    let sized = |id: AgentId, root: &str, names: &[&str]| {
        let mut entry = AgentEntry::new(id, root, root, "dist/index.js");
        entry.install_hosts = vec!["registry.test".to_owned()];
        let download_bytes = names
            .iter()
            .map(|name| u64::try_from(tarballs[name].len()).unwrap())
            .sum();
        entry.sizes = vec![AgentSizes {
            os: platform.os,
            arch: platform.arch,
            download_bytes,
            unpacked_bytes: unpacked,
        }];
        entry
    };
    let agents = vec![
        sized(agent_a(), "adapter-a", &A_PACKAGES),
        sized(agent_b(), "adapter-b", &B_PACKAGES),
    ];
    Snapshot::new(&package_json, &lock, base, node, agents).unwrap()
}

/// Lowercase hex of `bytes`.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").unwrap();
        out
    })
}

/// A `ManualTimer` the test and `Agents` share, recording every wait armed.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedTimer {
    manual: Arc<ManualTimer>,
    armed: Arc<Mutex<Vec<Duration>>>,
}

impl Timer for SharedTimer {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        self.armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(duration);
        self.manual.after(duration)
    }
}

/// The install's own waits: back-off and offline. `node --version` (10 s) and npm (10 min)
/// limits are left out.
pub(crate) const INSTALL_WAITS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(30),
];

impl SharedTimer {
    /// Every wait armed so far, in order.
    pub(crate) fn armed(&self) -> Vec<Duration> {
        self.armed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The install's own waits armed so far (back-off and offline), in order.
    pub(crate) fn install_waits(&self) -> Vec<Duration> {
        self.armed()
            .into_iter()
            .filter(|wait| INSTALL_WAITS.contains(wait))
            .collect()
    }

    /// Blocks until a wait of `duration` is armed (passing over the others), then ends it.
    pub(crate) fn fire_next(&self, duration: Duration) {
        loop {
            let armed = futures::executor::block_on(self.manual.next_armed());
            if armed == duration {
                assert!(self.manual.fire(), "the {duration:?} wait was dropped");
                return;
            }
        }
    }
}

/// A clock whose monotonic reading moves 250 ms per call.
#[derive(Debug, Default)]
pub(crate) struct StepClock(AtomicU64);

impl Clock for StepClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)
    }

    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.0.fetch_add(250, Ordering::SeqCst))
    }
}

/// One test's registry, data folder, timer and config.
pub(crate) struct Fixture {
    pub(crate) registry: TestRegistry,
    pub(crate) lookups: Lookups,
    pub(crate) data: TempDir,
    pub(crate) timer: SharedTimer,
    pub(crate) config: AgentsConfig,
    pub(crate) tarballs: BTreeMap<&'static str, Vec<u8>>,
}

impl Fixture {
    /// The fixture on [`BASE`], with the real Node as the user's.
    pub(crate) fn new(label: &str) -> Fixture {
        Fixture::with_base(label, BASE)
    }

    /// The fixture with tarball URLs on `base`; every host name resolves to the registry.
    pub(crate) fn with_base(label: &str, base: &str) -> Fixture {
        let registry = TestRegistry::start().unwrap();
        let tarballs = tarballs();
        for (name, bytes) in &tarballs {
            registry.route(&tarball_path(name), bytes.clone());
        }
        let (resolver, lookups) = resolve_all_to(registry.addr());
        let data = TempDir::new(label).unwrap();
        let mut config = AgentsConfig::new(data.path().to_owned(), platform());
        config.snapshot = snapshot(base, node_entry(None), 10_000);
        config.install.resolver = Some(resolver);
        config.install.allow_http_for_tests = true;
        config.node_search = user_node_search();
        config.clock = Arc::new(StepClock::default());
        Fixture {
            registry,
            lookups,
            data,
            timer: SharedTimer::default(),
            config,
            tarballs,
        }
    }

    /// Starts `Agents` on the fixture and reads each agent's first state.
    pub(crate) fn start(&self) -> (Agents, Events) {
        let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
        let (agents, events) = Agents::start(self.config.clone(), launcher, self.timer.clone());
        (agents, Events(events))
    }

    /// Starts `Agents` on the fixture with a launcher that calls `before` with each spec just
    /// before it starts it, and reads each agent's first state.
    pub(crate) fn start_hooked(
        &self,
        before: impl Fn(&LaunchSpec) + Send + Sync + 'static,
    ) -> (Agents, Events) {
        let launcher: Arc<dyn Launcher> = Arc::new(Hooked {
            inner: PassthroughLauncher::new(SpawnOptions::new()),
            before,
        });
        let (agents, events) = Agents::start(self.config.clone(), launcher, self.timer.clone());
        (agents, Events(events))
    }

    /// The requests made for package `name`'s tarball.
    pub(crate) fn requests_for(&self, name: &str) -> Vec<clauseo_agents::testing::Request> {
        let path = tarball_path(name);
        self.registry
            .requests()
            .into_iter()
            .filter(|request| request.path == path)
            .collect()
    }

    /// `<data>/agents/<id>/install`.
    pub(crate) fn install_root(&self, agent: &AgentId) -> PathBuf {
        clauseo_agents::install_root(self.data.path(), agent)
    }
}

/// The passthrough launcher, calling `before` with each spec first.
struct Hooked<F> {
    inner: PassthroughLauncher,
    before: F,
}

impl<F: Fn(&LaunchSpec) + Send + Sync> Launcher for Hooked<F> {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        (self.before)(spec);
        self.inner.launch(spec)
    }
}

/// A search that finds the real Node on `PATH` and nothing else.
pub(crate) fn user_node_search() -> NodeSearch {
    let (program, _) = real_node();
    let mut search = NodeSearch::none();
    search.path = Some(program.parent().unwrap().as_os_str().to_owned());
    search
}

/// The events' receiver, read by blocking.
pub(crate) struct Events(pub(crate) async_channel::Receiver<AgentsEvent>);

impl Events {
    /// The next event.
    pub(crate) fn next(&self) -> (AgentId, InstallState) {
        let event = futures::executor::block_on(self.0.recv()).expect("the events closed");
        let AgentsEvent::Install { agent, state } = event else {
            panic!("an event of another kind: {event:?}");
        };
        (agent, state)
    }

    /// Events up to and including the first of `agent`'s for which `done` holds.
    pub(crate) fn until(
        &self,
        agent: &AgentId,
        done: impl Fn(&InstallState) -> bool,
    ) -> Vec<(AgentId, InstallState)> {
        let mut seen = Vec::new();
        loop {
            let (id, state) = self.next();
            let last = id == *agent && done(&state);
            seen.push((id, state));
            if last {
                return seen;
            }
        }
    }

    /// Events up to `agent`'s `Ready` or `Failed`, and that last state.
    pub(crate) fn until_end(
        &self,
        agent: &AgentId,
    ) -> (Vec<(AgentId, InstallState)>, InstallState) {
        let seen = self.until(agent, |state| {
            matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
        });
        let last = seen.last().unwrap().1.clone();
        (seen, last)
    }

    /// Both agents' first states, A's then B's.
    pub(crate) fn first_states(&self) -> (InstallState, InstallState) {
        let (a, first) = self.next();
        let (b, second) = self.next();
        assert_eq!((a, b), (agent_a(), agent_b()));
        (first, second)
    }

    /// Every event already sent, without waiting.
    pub(crate) fn drain(&self) -> Vec<(AgentId, InstallState)> {
        std::iter::from_fn(|| self.0.try_recv().ok())
            .filter_map(|event| match event {
                AgentsEvent::Install { agent, state } => Some((agent, state)),
                _ => None,
            })
            .collect()
    }
}

/// `agent`'s states among `events`.
pub(crate) fn states_of(events: &[(AgentId, InstallState)], agent: &AgentId) -> Vec<InstallState> {
    events
        .iter()
        .filter(|(id, _)| id == agent)
        .map(|(_, state)| state.clone())
        .collect()
}

/// Every symbolic link or junction under `dir`, not following any.
pub(crate) fn links_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.file_type().is_symlink() {
                found.push(path);
            } else if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    found
}
