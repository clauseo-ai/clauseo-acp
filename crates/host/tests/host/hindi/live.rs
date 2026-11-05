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
use clauseo_process::{
    ExitReport, LaunchSpec, Launched, PassthroughLauncher, SpawnOptions, spawn_in_tree,
};
use clauseo_settings::{Effective, Env, Locations, PlatformDirs as _, SystemDirs, SystemFiles};
use futures::AsyncReadExt as _;

use super::flow::{self, Wanted};
use crate::support::NoReader;
use crate::write;

/// The variable naming the live test's own folder; it must not be the app's data folder.
pub(crate) const ROOT_VAR: &str = "CLAUSEO_HINDI_LIVE_DATA";

/// The variable naming the `node` that runs the pinned adapters' `npm ci` (route 2).
pub(crate) const NODE_VAR: &str = "CLAUSEO_HINDI_NODE";

/// Where an agent's adapter may be found.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Adapter {
    /// The variable that may name its executable, such as `CLAUSEO_HINDI_CLAUDE_ACP`.
    pub(crate) var: &'static str,
    /// Its package's script under an `npm ci` of the pins file, as G16 runs it.
    pub(crate) script: &'static str,
}

/// The pins file the adapters' versions live in (one home: G16's), from the repository root.
const PINS: &str = "crates/limits/tests/live";

/// The pins file's two files, which `npm ci` reads from the folder it runs in.
const PIN_FILES: [&str; 2] = ["package.json", "package-lock.json"];

/// How the adapter was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// The executable the adapter's variable names.
    Executable,
    /// The pinned packages, installed into the test's folder by `npm ci` on this run.
    PinnedInstall,
}

/// The program that starts the adapter, with its arguments, and the way it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Command {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) route: Route,
}

/// One live run of `adapter` with `env` as the settings entry's variables. Prints which way
/// it found the adapter and the record; fails unless the rule passes.
pub(crate) fn run(wanted: Wanted, adapter: Adapter, env: &BTreeMap<String, String>) {
    let root = root();
    let data = root.join("data");
    let matters = root.join("matters");
    write::create_dir_all(&data);
    write::remove_dir_all(&matters).unwrap();
    write::create_dir_all(&matters);
    let command = find(adapter, &root);
    let route = match command.route {
        Route::Executable => format!("the executable {} names", adapter.var),
        Route::PinnedInstall => "the pinned packages, installed by npm ci just now".to_owned(),
    };
    eprintln!(
        "adapter ({route}): {} {}",
        command.program,
        command.args.join(" ")
    );
    let settings = serde_json::json!({
        "agents": { wanted.agent: {
            "command": command.program,
            "args": command.args,
            "env": env,
        } }
    });
    write::write(
        &data.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
    );
    let locations = Locations::under_home(&root);
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

/// The adapter's command, the first way that works (see [`choose`]); panics with its error.
fn find(adapter: Adapter, root: &Path) -> Command {
    let executable = std::env::var_os(adapter.var).map(PathBuf::from);
    let node = std::env::var_os(NODE_VAR).map(PathBuf::from);
    choose(adapter, root, executable, node).unwrap_or_else(|error| panic!("{error}"))
}

/// The adapter's command, the first way that works:
/// 1. `executable`, the executable the adapter's variable names, by its absolute path;
/// 2. `node` (`CLAUSEO_HINDI_NODE`) with the npm beside it: `npm ci` of the pinned packages
///    into `<root>/adapters`, then that install's script run by `node` ([`install`]);
/// 3. none: an error with the two ways to choose from. The app's own installer (S17,
///    `clauseo-agents`, wired in by R20) is not on main, so the test cannot install through
///    it; once it is, it becomes the second way.
pub(crate) fn choose(
    adapter: Adapter,
    root: &Path,
    executable: Option<PathBuf>,
    node: Option<PathBuf>,
) -> Result<Command, String> {
    if let Some(path) = executable {
        if path.is_absolute() && path.is_file() {
            return Ok(Command {
                program: path.display().to_string(),
                args: Vec::new(),
                route: Route::Executable,
            });
        }
        return Err(format!(
            "{} must name the adapter's executable by its absolute path; it names {}",
            adapter.var,
            path.display()
        ));
    }
    match node {
        Some(node) if node.is_absolute() && node.is_file() => install(adapter, root, &node),
        Some(node) => Err(format!(
            "{NODE_VAR} must name node by its absolute path; it names {}",
            node.display()
        )),
        None => Err(format!(
            "no adapter for {var}. The app's installer (S17) is not on main yet, so the test \
             cannot install it. Either:\n  1. export {var}=/absolute/path/to/the/adapter, or\n  \
             2. export {NODE_VAR}=/absolute/path/to/node (npm must sit beside it); the test then \
             runs the pinned install into {dir} itself.",
            var = adapter.var,
            dir = root.join("adapters").display(),
        )),
    }
}

/// Route 2: copies the pins into `<root>/adapters` and runs `npm ci` there, with the npm beside
/// `node`, by its absolute path, and `node`'s folder first on `PATH` (npm's script starts `node`
/// by name). Fails with the tail of npm's output when it fails. Then names the adapter's
/// script, which the install made, to run with `node`.
pub(crate) fn install(adapter: Adapter, root: &Path, node: &Path) -> Result<Command, String> {
    let adapters = root.join("adapters");
    write::create_dir_all(&adapters);
    let pins = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(PINS);
    for file in PIN_FILES {
        let from = pins.join(file);
        let bytes = std::fs::read(&from)
            .map_err(|error| format!("reading the pins {}: {error}", from.display()))?;
        write::write(&adapters.join(file), &bytes);
    }
    let npm = npm_beside(node)?;
    let folder = node.parent().unwrap_or(node);
    let env = BTreeMap::from([(OsString::from("PATH"), path_with(folder))]);
    let args = vec![
        OsString::from("ci"),
        OsString::from("--prefix"),
        adapters.clone().into_os_string(),
    ];
    eprintln!(
        "running the pinned install (npm ci in {}) with {}",
        adapters.display(),
        node.display()
    );
    let spec = LaunchSpec::new(npm.clone(), args, env, adapters.clone())
        .map_err(|error| format!("npm ci: {error}"))?;
    let launched = spawn_in_tree(&spec, &SpawnOptions::new())
        .map_err(|error| format!("starting {}: {error}", npm.display()))?;
    let report = wait_for_exit(launched);
    if report.code() != Some(0) {
        return Err(format!(
            "npm ci of the pinned adapters failed (exit code {:?}, signal {:?}); its output \
             ends:\n{}",
            report.code(),
            report.signal(),
            report.stderr_tail(),
        ));
    }
    let script = adapters.join("node_modules").join(adapter.script);
    if !script.is_file() {
        return Err(format!(
            "npm ci finished but {} is missing",
            script.display()
        ));
    }
    Ok(Command {
        program: node.display().to_string(),
        args: vec![script.display().to_string()],
        route: Route::PinnedInstall,
    })
}

/// Drains the child's stdout and stderr while it runs, then waits for it. A pipe that fails
/// ends its output early; the exit code still says whether the install worked.
fn wait_for_exit(launched: Launched) -> ExitReport {
    let Launched {
        stdin,
        mut stdout,
        stderr,
        mut handle,
        ..
    } = launched;
    drop(stdin);
    futures::executor::block_on(async {
        let mut sink = Vec::new();
        let pipes = futures::join!(stdout.read_to_end(&mut sink), stderr.drain());
        drop(pipes);
        handle.exited().await
    })
}

/// The `npm` beside `node`, by its absolute path. On Windows npm is `npm.cmd`, a batch file the
/// process crate refuses, so route 2 is for macOS and Linux.
fn npm_beside(node: &Path) -> Result<PathBuf, String> {
    if cfg!(windows) {
        return Err(format!(
            "{NODE_VAR} cannot run the pinned install on Windows: npm there is a batch file. \
             Set the adapter's variable to its executable instead."
        ));
    }
    node.parent()
        .map(|folder| folder.join("npm"))
        .filter(|npm| npm.is_file())
        .ok_or_else(|| {
            format!(
                "no npm beside {}; the pinned install runs the npm next to node",
                node.display()
            )
        })
}

/// `folder` first on `PATH`, then the inherited `PATH`. This only prepends: nothing walks
/// `PATH` (the repository walks it in one place only, `xtask/src/process.rs`).
fn path_with(folder: &Path) -> OsString {
    let mut path = folder.as_os_str().to_owned();
    if let Some(inherited) = std::env::var_os("PATH") {
        path.push(if cfg!(windows) { ";" } else { ":" });
        path.push(inherited);
    }
    path
}

/// The live test's own folder, as [`prepare_root`] makes it, against the app's data folder.
fn root() -> PathBuf {
    let app = Locations::resolve(&SystemDirs, &NoEnv)
        .unwrap_or_else(|e| panic!("the app's data folder: {e}"));
    prepare_root(std::env::var_os(ROOT_VAR), app.data_dir())
        .unwrap_or_else(|error| panic!("{error}"))
}

/// `named` (`CLAUSEO_HINDI_LIVE_DATA`) as the live test's own folder: absolute, not the app's
/// data folder, not inside it and not holding it. Created when absent. The check comes before
/// the creation, so a refused name creates nothing.
pub(crate) fn prepare_root(named: Option<OsString>, app_data: &Path) -> Result<PathBuf, String> {
    let Some(root) = named.map(PathBuf::from) else {
        return Err(format!(
            "set {ROOT_VAR} to a folder of its own, such as ~/clauseo-hindi-live"
        ));
    };
    if !root.is_absolute() {
        return Err(format!(
            "{ROOT_VAR} must be an absolute path; it is {}",
            root.display()
        ));
    }
    if clauseo_matters::same_or_nested(&root, app_data) {
        return Err(format!(
            "{ROOT_VAR} overlaps the app's data folder {}; pick a folder of its own",
            app_data.display()
        ));
    }
    if root.is_file() {
        return Err(format!(
            "{ROOT_VAR} names a file, not a folder: {}",
            root.display()
        ));
    }
    write::create_dir_all(&root);
    Ok(root)
}

/// An environment with no variables, so `Locations::resolve` gives the app's default data
/// folder whatever `CLAUSEO_DATA_DIR` says.
struct NoEnv;

impl Env for NoEnv {
    fn var(&self, _name: &str) -> Option<OsString> {
        None
    }
}
