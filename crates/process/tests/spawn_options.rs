//! What [`SpawnOptions`] gives the child, seen from inside it: the agent is this test binary run
//! again (`support/self_agent.rs`), and it says on stderr whether it inherited a variable of
//! this test process and, on Windows, whether it shares this test's console. On every OS: the
//! environment, inherited by default and only the spec's with `clear_env`. On Windows also: a
//! console of the agent's own (`CREATE_NO_WINDOW`), and creation flags that keep the other
//! options.

#[cfg(test)]
#[path = "support/self_agent.rs"]
mod self_agent;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write as _;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{Launched, SpawnOptions};
use futures::future::{Either, select};

/// The spec variable that names the inherited variable the agent looks for.
const PROBE_NAME: &str = "CLAUSEO_PROCESS_PROBE_NAME";
/// The spec variable that holds this test process's pid.
const PROBE_PARENT: &str = "CLAUSEO_PROCESS_PROBE_PARENT";
/// How long the agent has to say what it found. A bound, so an agent that never runs (left
/// suspended on Windows) fails the test instead of hanging it.
const DEADLINE: Duration = Duration::from_secs(10);

/// The agent the tests here start, not a check: run as one, it writes one stderr line saying
/// whether the variable [`PROBE_NAME`] names is set and whether its console holds the process
/// [`PROBE_PARENT`] names, then returns. In a normal run it returns at once.
#[test]
fn agent_reports_its_environment_and_console() {
    if !self_agent::is_agent() {
        return;
    }
    let name = std::env::var_os(PROBE_NAME).expect("the probe's name");
    let parent: u32 = std::env::var(PROBE_PARENT)
        .expect("the parent's pid")
        .parse()
        .expect("a pid");
    let inherited = std::env::var_os(name).is_some();
    let shares = shares_console(parent);
    let mut stderr = std::io::stderr().lock();
    writeln!(
        stderr,
        "agent: inherited={inherited} shares_console={shares}"
    )
    .expect("write");
    stderr.flush().expect("flush stderr");
}

/// Whether process `pid` is attached to this process's console. With no console, it is not.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to GetConsoleProcessList, for the test agent"
)]
fn shares_console(pid: u32) -> bool {
    let mut pids = vec![0_u32; 1024];
    // SAFETY: the slice is live and writable for its whole length, which the binding passes.
    let n = unsafe { windows::Win32::System::Console::GetConsoleProcessList(&mut pids) };
    // A count past the slice means the list did not fit; then it is not checked.
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    n <= pids.len() && pids[..n].contains(&pid)
}

/// Off Windows there is no console to share.
#[cfg(all(test, not(windows)))]
fn shares_console(_pid: u32) -> bool {
    false
}

/// What the agent found.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    inherited: bool,
    shares_console: bool,
}

/// One variable this test process inherited (the test runner sets several), by name.
#[cfg(test)]
fn inherited_name() -> OsString {
    std::env::vars_os()
        .map(|(name, _)| name)
        .find(|name| {
            let text = name.to_string_lossy();
            !text.is_empty() && !text.starts_with('=') && !text.starts_with("CLAUSEO_PROCESS_")
        })
        .expect("the test process has an environment")
}

/// Starts the agent with `opts` and returns what it found. Fails if it says nothing within
/// [`DEADLINE`] or exits other than 0.
#[cfg(test)]
fn seen_with(opts: &SpawnOptions) -> Seen {
    let env = BTreeMap::from([
        (OsString::from(PROBE_NAME), inherited_name()),
        (
            OsString::from(PROBE_PARENT),
            OsString::from(std::process::id().to_string()),
        ),
    ]);
    let Launched {
        stdin,
        stdout: _stdout,
        stderr,
        mut handle,
        ..
    } = self_agent::launch("agent_reports_its_environment_and_console", opts, env).unwrap();
    drop(stdin);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&lines);
    let pumped = stderr.lines(move |line| {
        sink.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
    });
    let pumped = block_on(async {
        match select(pin!(pumped), pin!(Timer::after(DEADLINE))).await {
            Either::Left((pumped, _)) => pumped,
            Either::Right(_) => panic!("the agent's stderr did not end within {DEADLINE:?}"),
        }
    });
    pumped.unwrap();
    let report = block_on(handle.exited());
    let lines = lines.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(report.code(), Some(0), "{lines:?}");
    let line = lines
        .iter()
        .find_map(|line| line.strip_prefix("agent: "))
        .unwrap_or_else(|| panic!("no report from the agent in {lines:?}"));
    Seen {
        inherited: line.contains("inherited=true"),
        shares_console: line.contains("shares_console=true"),
    }
}

/// With default options the child inherits this process's variables and, on Windows, has a
/// console of its own (`CREATE_NO_WINDOW`), not this test's. The console check can fail only
/// when this test has a console, as under nextest on CI.
#[test]
fn default_options_inherit_the_environment_and_give_a_console_of_its_own() {
    let seen = seen_with(&SpawnOptions::new());
    assert_eq!(
        seen,
        Seen {
            inherited: true,
            shares_console: false,
        }
    );
}

/// With `clear_env` the child gets none of this process's variables. On Windows, creation flags
/// given after it keep it, and a flag the crate already sets (`CREATE_NO_WINDOW`) stays set.
#[test]
fn clear_env_gives_none_of_the_parents_variables() {
    let opts = SpawnOptions::new().clear_env();
    #[cfg(windows)]
    let opts = opts.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    let seen = seen_with(&opts);
    assert_eq!(
        seen,
        Seen {
            inherited: false,
            shares_console: false,
        }
    );
}
