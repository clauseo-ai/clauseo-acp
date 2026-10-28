//! Starts `fake_agent` on a scenario through the passthrough launcher, in a folder of the run's
//! own, and turns its stderr into lines a test can wait for.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_process::{
    Error, LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, StderrPump,
};
use futures::StreamExt;
use futures::channel::mpsc;

/// This run's folder under the temp folder: the agent's working directory, removed when the run
/// ends. Nextest runs each test in its own process.
pub(crate) struct RunDir(PathBuf);

impl RunDir {
    pub(crate) fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("clauseo-process-pipes-{}", std::process::id()));
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

/// `fake_agent --scenario <scenario>`, started in `dir` the way `host` will start an agent:
/// through an `Arc<dyn Launcher>`.
pub(crate) fn launch(scenario: &str, dir: &RunDir) -> Result<Launched, Error> {
    let bin = clauseo_fake_agent::bin_path().expect("fake_agent binary");
    launch_program(bin, &["--scenario", scenario], dir)
}

/// `program` with `args`, started in `dir`.
pub(crate) fn launch_program(
    program: PathBuf,
    args: &[&str],
    dir: &RunDir,
) -> Result<Launched, Error> {
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
