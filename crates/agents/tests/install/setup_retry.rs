//! A set-up step that fails once (the Windows VM pass of #664: an `npm ci` refused by a file
//! lock, likely an antivirus scanning the files npm has just written) is retried after the 2 s
//! backoff from the checked downloads already on disk, with no second request for any file, and
//! the app log names the failed step, npm's exit code and the classified cause.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_agents::testing::trace;
use clauseo_agents::{Agents, InstallState};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use tracing_subscriber::fmt::MakeWriter;

use crate::support::{A_PACKAGES, Events, Fixture, agent_a};

/// The first `npm ci` in its place: npm's own error line for a locked file, and exit 1.
const LOCKED_NPM: &str = "console.error('npm error code EBUSY'); process.exit(1);";

/// The passthrough launcher with the first `npm ci` replaced by [`LOCKED_NPM`].
struct FailOnce {
    inner: PassthroughLauncher,
    ci_runs: AtomicUsize,
}

impl Launcher for FailOnce {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let is_ci = spec.args().iter().any(|arg| arg == "ci");
        if !is_ci || self.ci_runs.fetch_add(1, Ordering::SeqCst) > 0 {
            return self.inner.launch(spec);
        }
        trace(format_args!(
            "launch: a locked npm in place of the first npm ci"
        ));
        let args: Vec<OsString> = vec!["--single-threaded".into(), "-e".into(), LOCKED_NPM.into()];
        let env: BTreeMap<OsString, OsString> = spec.env().clone();
        let fake = LaunchSpec::new(spec.program().to_owned(), args, env, spec.cwd().to_owned())?;
        self.inner.launch(&fake)
    }
}

/// The app log's text, from every thread.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Log {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Log {
    type Writer = Log;

    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

#[test]
fn a_set_up_that_fails_once_is_retried_with_no_second_download_and_logged() {
    let log = Log::default();
    // Global, as the job runs on its own thread; nextest runs each test in its own process.
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish(),
    )
    .unwrap();
    let fx = Fixture::new("setup-retry");
    let launcher: Arc<dyn Launcher> = Arc::new(FailOnce {
        inner: PassthroughLauncher::new(SpawnOptions::new()),
        ci_runs: AtomicUsize::new(0),
    });
    let (agents, events) = Agents::start(fx.config.clone(), launcher, fx.timer.clone());
    let events = Events(events);
    events.first_states();
    agents.install(&agent_a());
    fx.timer.fire_next(Duration::from_secs(2));
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(fx.timer.install_waits(), [Duration::from_secs(2)]);
    for name in A_PACKAGES {
        assert_eq!(fx.requests_for(name).len(), 1, "{name} fetched again");
    }
    let text = log.text();
    assert!(
        text.contains("install: try failed at npm install: file in use or refused"),
        "{text}"
    );
    assert!(text.contains("transient, retrying"), "{text}");
    assert!(text.contains("npm_exit=1"), "{text}");
    assert!(text.contains("attempt=1"), "{text}");
    assert!(
        text.contains("install: set-up discarded: temp folder removed after the try failed"),
        "{text}"
    );
}
