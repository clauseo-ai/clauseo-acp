//! The `stetdoc-slow-test` build (`Cargo.toml`; never in a release, which release.yml checks):
//! stetdoc's background tries ([`super::recheck_later`]) start a sleep in stetdoc's place for
//! their first `CLAUSEO_TEST_STETDOC_SLOW_TRIES` launches (none when unset or not a number), as
//! a stetdoc the OS holds while it scans it. Each try's real limit kills its sleep, so the
//! tries time out with the same log lines and timing as a held stetdoc. With 2 on a first run
//! (no "seen" marker), tries 1 and 2 time out at 20 s and 40 s, and try 3 answers at about
//! 40 s, after the first conversation's launch has gone on without the skill. Only the
//! background tries are wrapped: the start's own try, for a stetdoc seen before, is not.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use clauseo_process::{Error, LaunchSpec, Launched, Launcher};

/// The variable the app reads, in this build only, for how many launches to hold.
const SLOW_TRIES_VAR: &str = "CLAUSEO_TEST_STETDOC_SLOW_TRIES";

/// How long the sleep would last: far past any try's limit, which kills it first.
const SLEEP_SECS: u32 = 3600;

/// Bytes only this build holds: release.yml's scan of every app binary it ships fails when it
/// finds them, so this code in a release binary is caught whatever its own strings. Read at
/// each launch, so the linker keeps it.
#[used]
static STETDOC_SLOW_MARKER: [u8; 32] = *b"CLAUSEO-STETDOC-SLOW-TEST-MARKER";

/// A launcher that starts the sleep in place of the first `held` specs, then each as it is.
pub(super) struct Slow<L> {
    inner: L,
    held: Mutex<usize>,
}

impl<L: Launcher> Slow<L> {
    /// `inner` holding as many launches as [`SLOW_TRIES_VAR`] says.
    pub(super) fn from_env(inner: L) -> Self {
        let held = std::env::var(SLOW_TRIES_VAR).ok();
        Self::new(inner, tries(held.as_deref()))
    }

    /// `inner` holding the first `held` launches.
    pub(super) fn new(inner: L, held: usize) -> Self {
        Self {
            inner,
            held: Mutex::new(held),
        }
    }

    /// Whether this launch is held: one fewer is left to hold after it.
    fn take(&self) -> bool {
        let mut left = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let held = *left > 0;
        *left = left.saturating_sub(1);
        held
    }
}

impl<L: Launcher> Launcher for Slow<L> {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        std::hint::black_box(&STETDOC_SLOW_MARKER);
        if !self.take() {
            return self.inner.launch(spec);
        }
        tracing::warn!("stetdoc-slow-test: a sleep runs in stetdoc's place");
        self.inner.launch(&sleep_for(spec)?)
    }
}

/// The count in `value` ([`SLOW_TRIES_VAR`]'s); 0 when unset or not a number.
fn tries(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

/// The sleep, with `spec`'s environment and folder: `/bin/sleep`, or on Windows `ping` to
/// itself once a second (`PING.EXE` under the system folder, `SystemRoot`).
fn sleep_for(spec: &LaunchSpec) -> Result<LaunchSpec, Error> {
    let (program, args) = sleep_command();
    LaunchSpec::new(program, args, spec.env().clone(), spec.cwd().to_path_buf())
}

#[cfg(not(windows))]
fn sleep_command() -> (PathBuf, Vec<OsString>) {
    (
        PathBuf::from("/bin/sleep"),
        vec![OsString::from(SLEEP_SECS.to_string())],
    )
}

#[cfg(windows)]
fn sleep_command() -> (PathBuf, Vec<OsString>) {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    let program = PathBuf::from(root).join("System32").join("PING.EXE");
    let count = (SLEEP_SECS + 1).to_string();
    let args = ["-n", count.as_str(), "127.0.0.1"].map(OsString::from);
    (program, Vec::from(args))
}

#[cfg(test)]
mod tests {
    use clauseo_process::{PassthroughLauncher, SpawnOptions};

    use super::*;

    #[test]
    fn the_count_is_the_variable_s_number_and_0_otherwise() {
        assert_eq!(tries(Some("2")), 2);
        assert_eq!(tries(Some(" 3\n")), 3);
        assert_eq!(tries(Some("two")), 0);
        assert_eq!(tries(None), 0);
    }

    /// The first `held` launches are held and the later ones are not.
    #[test]
    fn only_the_first_launches_are_held() {
        let slow = Slow::new(PassthroughLauncher::new(SpawnOptions::new()), 2);
        assert_eq!([slow.take(), slow.take(), slow.take()], [true, true, false]);
    }

    /// The sleep keeps the spec's environment and folder, and runs far past any try's limit.
    #[cfg(unix)]
    #[test]
    fn the_sleep_keeps_the_spec_s_environment_and_folder() {
        use std::collections::BTreeMap;

        let env = BTreeMap::from([(OsString::from("A"), OsString::from("1"))]);
        let folder = std::env::temp_dir();
        let spec = LaunchSpec::new(
            folder.join("stetdoc"),
            vec![OsString::from("--version")],
            env.clone(),
            folder.clone(),
        )
        .unwrap();
        let sleep = sleep_for(&spec).unwrap();
        assert_eq!(sleep.program(), std::path::Path::new("/bin/sleep"));
        assert_eq!(sleep.args(), [OsString::from("3600")]);
        assert_eq!(sleep.env(), &env);
        assert_eq!(sleep.cwd(), folder.as_path());
    }
}
