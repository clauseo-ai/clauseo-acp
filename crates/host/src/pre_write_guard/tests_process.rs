//! The guard as a real process: a normal refusal, a panic and the watchdog each end it with the
//! refusing exit code and a reason on stderr: exit 2, the code Claude Code and Codex block on.
//! The child is this test binary running the guard's real entry ([`super::run`], what the app's
//! `--pre-write-guard` reaches; the desktop binary is not built by the host's tests), started
//! through the process crate as the self-test starts a hook.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, Timer};
use futures::future::{Either, select};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};

use super::watchdog::{BROKEN, LATE, NO_WATCHDOG, PLANTED, TEST_DEADLINE_MS, UNCHECKED};
use crate::clock::{Clock, ProcessTimer, SystemClock};

/// The variable that makes [`guard_child`] run the guard.
const CHILD: &str = "CLAUSEO_GUARD_PROCESS_CHILD";

/// This test's own name, for the child's command line.
const CHILD_TEST: &str = "pre_write_guard::tests_process::guard_child";

/// The longest a child may take before the test fails: far over the planted deadline.
const WAIT: Duration = Duration::from_secs(60);

/// Not a test of its own: the guard's real entry in a child process the tests start, which
/// exits with the guard's code as the app does. It returns at once in any other run.
#[test]
fn guard_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let matter = std::env::temp_dir();
    let args: Vec<OsString> = vec![super::MATTER_FLAG.into(), matter.into()];
    let code = super::run(
        &args,
        std::io::stdin().lock(),
        std::io::sink(),
        std::io::stderr(),
    );
    super::watchdog::exit_now(code);
}

/// The child's exit code and stderr, given `input` on its stdin and `planted` (if any) as the
/// planted fault ([`PLANTED`]), with a 300 ms watchdog.
fn child(input: &[u8], planted: Option<&str>) -> (Option<i32>, String) {
    let mut env = BTreeMap::from([
        (OsString::from(CHILD), OsString::from("1")),
        (OsString::from(TEST_DEADLINE_MS), OsString::from("300")),
    ]);
    if let Some(planted) = planted {
        env.insert(OsString::from(PLANTED), OsString::from(planted));
    }
    let args = [CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"]
        .map(OsString::from)
        .to_vec();
    let program = std::env::current_exe().unwrap();
    let cwd: PathBuf = std::env::temp_dir();
    let spec = LaunchSpec::new(program, args, env, cwd).unwrap();
    let launcher = PassthroughLauncher::new(SpawnOptions::new().end_with_parent());
    let Launched {
        mut stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec).unwrap();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let timer = ProcessTimer(clock);
    let work = async {
        drop(stdin.write_all(input).await);
        drop(stdin.close().await);
        let (mut stdout, mut out) = (stdout, Vec::new());
        let reading = stdout.read_to_end(&mut out);
        let (_read, _drained) = futures::future::join(reading, stderr.drain()).await;
        let exit = handle.exited().await;
        (exit.code(), exit.stderr_tail())
    };
    futures::executor::block_on(async {
        match select(pin!(work), pin!(timer.after(WAIT))).await {
            Either::Left((ended, _)) => ended,
            Either::Right(((), _)) => panic!("the guard's process did not end within {WAIT:?}"),
        }
    })
}

/// A made-up `Write` the guard refuses for its unasked mode, whatever the matter.
fn refused_input() -> Vec<u8> {
    super::selftest::input(&std::env::temp_dir())
}

#[test]
fn a_normal_refusal_in_the_guard_process_exits_2_with_a_reason() {
    let (code, errors) = child(&refused_input(), None);
    assert_eq!(code, Some(super::DENY), "{errors}");
    assert!(!errors.trim().is_empty(), "{errors}");
}

#[test]
fn a_panic_in_the_guard_process_exits_2_with_a_reason() {
    let (code, errors) = child(&refused_input(), Some("panic"));
    assert_eq!(code, Some(super::DENY), "{errors}");
    assert!(errors.contains(BROKEN), "{errors}");
}

#[test]
fn the_guard_process_past_its_deadline_exits_2_with_a_reason() {
    let (code, errors) = child(&refused_input(), Some("hang"));
    assert_eq!(code, Some(super::DENY), "{errors}");
    assert!(errors.contains(LATE), "{errors}");
}

#[test]
fn every_refusal_without_an_answer_starts_with_the_shared_sentence() {
    for line in [LATE, BROKEN, NO_WATCHDOG] {
        let reason = line
            .strip_prefix(UNCHECKED)
            .unwrap_or_else(|| panic!("{line}"));
        assert!(reason.starts_with(' ') && reason.len() < 80, "{line}");
    }
}
