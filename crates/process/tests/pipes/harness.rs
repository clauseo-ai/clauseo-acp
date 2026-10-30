//! Starts `fake_agent` on a scenario through the passthrough launcher, in a folder of the run's
//! own, and turns its stderr into lines a test can wait for.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use clauseo_process::{
    Error, LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, StderrPump,
};
use futures::StreamExt;
use futures::channel::mpsc;

/// Numbers the run folders of one process, so tests that share it (`cargo test`) get one each.
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// This test's folder under the temp folder: the agent's working directory, removed when the
/// test ends. Named by the process and a counter, so it is the test's own under nextest (one
/// process per test) and under `cargo test` (one process for all).
pub(crate) struct RunDir(PathBuf);

impl RunDir {
    pub(crate) fn new() -> Self {
        let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("clauseo-process-pipes-{}-{n}", std::process::id()));
        crate::write::create_dir_all(&dir).expect("create the run folder");
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        drop(crate::write::remove_dir_all(&self.0));
    }
}

/// Every launch holds it, so a test that changes what a child inherits (SIGTERM ignored) keeps
/// other launches out meanwhile, under `cargo test` where the pipe tests share one process.
static LAUNCH: Mutex<()> = Mutex::new(());

/// The launch lock, for a test that changes what a child inherits around its own launch.
pub(crate) fn launch_lock() -> MutexGuard<'static, ()> {
    LAUNCH.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `fake_agent --scenario <scenario>`, started in `dir` the way `host` will start an agent:
/// through an `Arc<dyn Launcher>`.
pub(crate) fn launch(scenario: &str, dir: &RunDir) -> Result<Launched, Error> {
    launch_locked(&launch_lock(), scenario, dir)
}

/// As [`launch`], under the launch lock the caller holds.
pub(crate) fn launch_locked(
    _lock: &MutexGuard<'static, ()>,
    scenario: &str,
    dir: &RunDir,
) -> Result<Launched, Error> {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    start(bin, &["--scenario", scenario], dir)
}

/// `program` with `args`, started in `dir`.
pub(crate) fn launch_program(
    program: PathBuf,
    args: &[&str],
    dir: &RunDir,
) -> Result<Launched, Error> {
    let _lock = launch_lock();
    start(program, args, dir)
}

/// Starts `program` with `args` in `dir`, under the launch lock the caller holds.
fn start(program: PathBuf, args: &[&str], dir: &RunDir) -> Result<Launched, Error> {
    let args = args.iter().map(OsString::from).collect();
    let spec = LaunchSpec::new(program, args, BTreeMap::new(), dir.path().to_owned())?;
    let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    launcher.launch(&spec)
}

/// The agent's stderr lines as they come, and every line seen so far, for failure messages.
pub(crate) struct Lines {
    rx: mpsc::UnboundedReceiver<String>,
    seen: Vec<String>,
}

/// `pump` with each line sent to the returned [`Lines`]. Drive the future alongside the test.
pub(crate) fn lines(pump: StderrPump) -> (impl Future<Output = io::Result<()>>, Lines) {
    let (tx, rx) = mpsc::unbounded();
    let pumped = pump.lines(move |line| drop(tx.unbounded_send(line.to_owned())));
    let lines = Lines {
        rx,
        seen: Vec::new(),
    };
    (pumped, lines)
}

impl Lines {
    /// Waits for the next line that starts with `prefix` and returns it; fails if stderr ends
    /// first.
    pub(crate) async fn until(&mut self, prefix: &str) -> String {
        while let Some(line) = self.rx.next().await {
            self.seen.push(line.clone());
            if line.starts_with(prefix) {
                return line;
            }
        }
        panic!(
            "stderr ended before a line starting {prefix:?}; it had {:?}",
            self.seen
        );
    }

    /// The grandchild's pid, from the `fake_agent: child <pid>` line of `spawn_child`.
    pub(crate) async fn grandchild(&mut self) -> u32 {
        let line = self.until("fake_agent: child ").await;
        line.strip_prefix("fake_agent: child ")
            .and_then(|pid| pid.parse().ok())
            .unwrap_or_else(|| panic!("no pid in {line:?}"))
    }
}
