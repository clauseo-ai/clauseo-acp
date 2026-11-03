//! The live half: a host started as the desktop starts it, on a test data folder the person
//! running the test names, launching the real `claude-acp` or `codex-acp` adapter.
//!
//! As the desktop (`crates/desktop/src/lib.rs`, `host_config`): the effective settings with
//! the system policy sources, a `PassthroughLauncher` with the user's home as the agent's
//! home, the system's refused folders, the system clock. Unlike it: the data folder is
//! `<root>/data` under `CLAUSEO_HINDI_LIVE_DATA` (never the app's), no limits reader runs
//! (it would read the logins' plan usage, which this test has no business with), and no
//! pre-write guard hook goes (the guard is the desktop executable; this test writes nothing).
//!
//! The models go in the settings agent entry, the way the desktop reads them: Claude's
//! `ANTHROPIC_MODEL`, Codex's `CODEX_CONFIG` with `model` and `model_reasoning_effort` (the
//! two keys the host keeps from it, `agent/codex.rs`). Nothing here reads `~/.claude`,
//! `~/.codex`, a `.env` file or a key: the agent finds its own login, Claude through the
//! user's settings (Vertex), Codex in its home under the data folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::{Host, HostConfig};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_settings::{Effective, Env, Locations, PlatformDirs as _, SystemDirs, SystemFiles};

use super::flow::{self, Wanted};
use crate::support::NoReader;
use crate::write;

/// The variable naming the live test's own folder; it must exist, outside the app's data.
pub(crate) const ROOT_VAR: &str = "CLAUSEO_HINDI_LIVE_DATA";

/// Where an agent's adapter may be found.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Adapter {
    /// The variable that may name its executable, such as `CLAUSEO_HINDI_CLAUDE_ACP`.
    pub(crate) var: &'static str,
    /// Its package's script under an `npm ci` of the pins file, as G16 runs it.
    pub(crate) script: &'static str,
}

/// The pins file the adapters' versions live in (one home: G16's).
const PINS: &str = "crates/limits/tests/live";

/// One live run of `adapter` with `env` as the settings entry's variables. Prints which way
/// it found the adapter and the record; fails unless the rule passes.
pub(crate) fn run(wanted: Wanted, adapter: Adapter, env: &BTreeMap<String, String>) {
    let root = root();
    let data = root.join("data");
    let matters = root.join("matters");
    write::create_dir_all(&data);
    write::remove_dir_all(&matters).unwrap();
    write::create_dir_all(&matters);
    let (command, args) = find(adapter, &root);
    let settings = serde_json::json!({
        "agents": { wanted.agent: { "command": command, "args": args, "env": env } }
    });
    write::write(
        &data.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
    );
    let locations = Locations::under_root(&data);
    let home = locations.agent_home(wanted.agent);
    let sources = clauseo_settings::system_policy_sources();
    let sources: Vec<&dyn clauseo_settings::PolicySource> =
        sources.iter().map(AsRef::as_ref).collect();
    let effective = Effective::load(&locations, &SystemFiles, &sources);
    let mut spawn = SpawnOptions::new();
    if let Some(home) = SystemDirs.home() {
        spawn = spawn.home(home);
    }
    let config = HostConfig::new(effective, Arc::new(PassthroughLauncher::new(spawn)))
        .with_refused_folders(clauseo_settings::refused_folders(&SystemDirs))
        .with_limits_reader(Arc::new(NoReader));
    let (handle, join) = Host::start(config).unwrap_or_else(|e| panic!("Host::start: {e}"));
    let asked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        flow::ask(&handle, &matters, wanted)
    }));
    let stopped = futures::executor::block_on(handle.shutdown());
    drop(join.join());
    let asked = match asked {
        Ok(asked) => asked,
        Err(panic) => {
            if wanted.agent == "codex-acp" {
                eprintln!(
                    "If Codex is not signed in, sign in once in its home here, then run again:\n  \
                     CODEX_HOME={} codex login --device-auth",
                    home.display()
                );
            }
            std::panic::resume_unwind(panic);
        }
    };
    stopped.unwrap_or_else(|e| panic!("shutdown: {e}"));
    eprintln!(
        "{} with {}:\n{}",
        wanted.agent,
        wanted.model,
        asked.record()
    );
    assert!(asked.verdict.passed(), "{}", asked.verdict);
}

/// The adapter's command and arguments, the first way that works, printed:
/// 1. the executable `adapter.var` names;
/// 2. the pinned packages in `<root>/adapters` (an `npm ci` of [`PINS`]), run by `node`
///    (`CLAUSEO_HINDI_NODE`);
/// 3. none: the test fails with the steps for either. The app's own installer (S17,
///    `clauseo-agents`, wired in by R20) is not on main, so the test cannot install through
///    it; once it is, it becomes the second way.
fn find(adapter: Adapter, root: &Path) -> (String, Vec<String>) {
    if let Some(path) = std::env::var_os(adapter.var).map(PathBuf::from) {
        assert!(
            path.is_absolute() && path.is_file(),
            "{} must name the adapter's executable by its absolute path; it names {}",
            adapter.var,
            path.display()
        );
        eprintln!("adapter (way 1, {}): {}", adapter.var, path.display());
        return (path.display().to_string(), Vec::new());
    }
    let adapters = root.join("adapters");
    let script = adapters.join("node_modules").join(adapter.script);
    if script.is_file() {
        let node = node().unwrap_or_else(|| {
            panic!(
                "{} exists but no node: set CLAUSEO_HINDI_NODE",
                script.display()
            )
        });
        eprintln!(
            "adapter (way 2, the pinned install): {} {}",
            node.display(),
            script.display()
        );
        return (
            node.display().to_string(),
            vec![script.display().to_string()],
        );
    }
    panic!(
        "no adapter for {var}. The app's installer (S17) is not on main yet, so the test cannot \
         install it. Either:\n  1. export {var}=/absolute/path/to/the/adapter, or\n  2. install \
         the pinned adapters into the test's folder once (needs node and npm), from the \
         repository root, and set CLAUSEO_HINDI_NODE=$(command -v node):\n     mkdir -p {dir} && cp {PINS}/package.json {PINS}/package-lock.json \
         {dir}/ && npm ci --prefix {dir}",
        var = adapter.var,
        dir = adapters.display(),
    );
}

/// `CLAUSEO_HINDI_NODE`, when it names a file by its absolute path. No `PATH` walk: the
/// repository walks `PATH` in one place only (`xtask/src/process.rs`).
fn node() -> Option<PathBuf> {
    std::env::var_os("CLAUSEO_HINDI_NODE")
        .map(PathBuf::from)
        .filter(|node| node.is_absolute() && node.is_file())
}

/// `CLAUSEO_HINDI_LIVE_DATA`: an existing absolute folder that is not, does not hold and is
/// not inside the app's own data folder (as `Locations::resolve` finds it, ignoring
/// `CLAUSEO_DATA_DIR`).
fn root() -> PathBuf {
    let root = std::env::var_os(ROOT_VAR)
        .map(PathBuf::from)
        .filter(|root| root.is_absolute() && root.is_dir())
        .unwrap_or_else(|| {
            panic!("set {ROOT_VAR} to an existing folder of its own, such as ~/clauseo-hindi-live")
        });
    let app = Locations::resolve(&SystemDirs, &NoEnv)
        .unwrap_or_else(|e| panic!("the app's data folder: {e}"));
    assert!(
        !clauseo_matters::same_or_nested(&root, app.data_dir()),
        "{ROOT_VAR} overlaps the app's data folder {}; pick a folder of its own",
        app.data_dir().display()
    );
    root
}

/// An environment with no variables, so `Locations::resolve` gives the app's default data
/// folder whatever `CLAUSEO_DATA_DIR` says.
struct NoEnv;

impl Env for NoEnv {
    fn var(&self, _name: &str) -> Option<OsString> {
        None
    }
}
