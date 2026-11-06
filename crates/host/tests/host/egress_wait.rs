//! The run folder removal's wait (`launch_checks::straggler`): a session's proxy closes, and its
//! folder goes, only once the agent's tree and its Chrome are gone. A straggler that outlives
//! the wait's cap keeps the proxy and the folder until the host stops. Two sessions of one
//! matter close their proxies independently, and a panic in the removal still closes its proxy.
//!
//! The stragglers in the agent's group are Linux only: the test becomes its orphans' reaper
//! (`PR_SET_CHILD_SUBREAPER`), so the agent's helper, killed with the group, stays a zombie in
//! it until the test reaps it by pid.

use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::SessionKey;
use serde_json::{Value, json};

use crate::egress::{Recording, calls, claude_then, wired};
use crate::profile_rig::{CLAUDE, Wired, turn};
use crate::support::Dir;

/// The agent's last line: it exits once its turn has ended.
fn exit() -> Value {
    json!({"exit": {"code": 0}})
}

/// Waits `wait` with nothing to do.
#[cfg(target_os = "linux")]
fn pause(wait: Duration) {
    let (_keep, never) = channel::<()>();
    let _timed_out = never.recv_timeout(wait);
}

/// That no close comes within `wait`.
fn no_close_within(closes: &Receiver<SessionKey>, wait: Duration) {
    assert!(
        closes.recv_timeout(wait).is_err(),
        "the proxy closed while the session's processes were still there"
    );
}

#[cfg(target_os = "linux")]
mod group {
    //! The agent's helper as a straggler in its group.

    use std::path::Path;
    use std::time::Duration;

    use nix::sys::wait::waitpid;
    use nix::unistd::Pid;
    use serde_json::{Value, json};

    use super::pause;

    /// The file, in the matter folder (the agent's working folder), the helper's pid goes in.
    pub(super) const PID_FILE: &str = "helper.pid";

    /// The scenario step that starts the helper in the agent's group, its pid in [`PID_FILE`].
    pub(super) fn helper() -> Value {
        json!({"spawn_child": {"pid_file": PID_FILE, "own_group": false}})
    }

    /// Makes this test process the reaper of its orphans, so the agent's helper, orphaned when
    /// the agent exits, stays a zombie in the agent's group until [`reap`].
    pub(super) fn become_reaper() {
        nix::sys::prctl::set_child_subreaper(true).unwrap();
    }

    /// The helper's pid, once the matter folder `folder` holds its file.
    pub(super) fn pid(folder: &Path) -> Pid {
        let pid_file = folder.join(PID_FILE);
        for _ in 0..600 {
            if let Ok(text) = std::fs::read_to_string(&pid_file)
                && let Ok(pid) = text.trim().parse()
            {
                return Pid::from_raw(pid);
            }
            pause(Duration::from_millis(50));
        }
        panic!("the agent did not start its helper");
    }

    /// Waits until `pid` is a zombie: the agent has exited and its tree's kill reached it.
    pub(super) fn until_zombie(pid: Pid) {
        let stat = format!("/proc/{pid}/stat");
        for _ in 0..600 {
            let text = std::fs::read_to_string(&stat).unwrap_or_default();
            // `pid (comm) state ...`: the state follows the last `)`.
            if text
                .rsplit(')')
                .next()
                .is_some_and(|rest| rest.trim_start().starts_with('Z'))
            {
                return;
            }
            pause(Duration::from_millis(50));
        }
        panic!("the helper did not become a zombie");
    }

    /// Reaps `pid`: the agent's group is gone.
    pub(super) fn reap(pid: Pid) {
        waitpid(pid, None).unwrap();
    }
}

/// A run folder's path from its first launch.
#[cfg(target_os = "linux")]
fn run_folder(launcher: &Wired) -> std::path::PathBuf {
    launcher.seen()[0].run.clone().expect("a run folder")
}

/// A helper left in the agent's group after the agent exits holds the session's proxy and
/// folder: the removal (and with it the proxy's close) comes only once the group is gone.
#[cfg(target_os = "linux")]
#[test]
fn the_proxy_closes_only_once_the_agents_group_is_gone() {
    group::become_reaper();
    let dir = Dir::new("egress_group_gone", &[]);
    claude_then(
        &dir,
        &json!([{"name": "browser"}]),
        &[group::helper(), exit()],
    );
    let (proxy, log, closes) = Recording::new(false);
    let launcher = Wired::new(&dir);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        wired(config, proxy, removals)
    });
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let helper = group::pid(&folder);
    group::until_zombie(helper);
    no_close_within(&closes, Duration::from_secs(1));
    assert_eq!(
        calls(&log),
        ["open 1"],
        "nothing freed while the group is there"
    );
    assert!(run_folder(&launcher).exists());
    group::reap(helper);
    closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the proxy closed once the group was gone");
    assert_eq!(calls(&log), ["open 1", "removal", "close 1"]);
    assert!(!run_folder(&launcher).exists());
    host.shutdown();
}

/// A helper that outlives the wait's cap: nothing is freed, the folder stays, and the proxy is
/// held until the host stops.
#[cfg(target_os = "linux")]
#[test]
fn a_straggler_past_the_cap_holds_the_proxy_and_folder_until_the_host_stops() {
    group::become_reaper();
    let dir = Dir::new("egress_past_cap", &[]);
    claude_then(
        &dir,
        &json!([{"name": "browser"}]),
        &[group::helper(), exit()],
    );
    let (proxy, log, closes) = Recording::new(false);
    let launcher = Wired::new(&dir);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        wired(config, proxy, removals).with_tree_wait(Duration::from_millis(300))
    });
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let helper = group::pid(&folder);
    group::until_zombie(helper);
    // Well past the cap: the removal has given up and holds the session.
    no_close_within(&closes, Duration::from_secs(2));
    assert_eq!(calls(&log), ["open 1"]);
    let folder = run_folder(&launcher);
    assert!(folder.exists(), "the folder stays past the cap");
    host.shutdown();
    assert_eq!(
        calls(&log),
        ["open 1", "close 1"],
        "closed at stop, not removed"
    );
    assert!(folder.exists(), "the next start's sweep takes it");
    group::reap(helper);
}

/// Something listening on the session's Chrome debugging port after the agent has exited, as a
/// Chrome that leads a group of its own would, holds the proxy until it stops listening.
#[test]
fn a_listener_on_the_chrome_port_holds_the_proxy_past_the_agents_exit() {
    let dir = Dir::new("egress_cdp", &[]);
    claude_then(&dir, &json!([{"name": "browser"}]), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let (bound, listener) = channel::<TcpListener>();
    let bound = Mutex::new(bound);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_tree_wait_hook(Arc::new(move |cdp| {
            let port = cdp.expect("the session claimed a Chrome port");
            let chrome = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
            bound.lock().unwrap().send(chrome).unwrap();
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let chrome = listener
        .recv_timeout(Duration::from_secs(30))
        .expect("the removal began its wait");
    no_close_within(&closes, Duration::from_secs(1));
    assert_eq!(calls(&log), ["open 1"]);
    drop(chrome);
    closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the proxy closed once Chrome's port refused");
    assert_eq!(calls(&log), ["open 1", "removal", "close 1"]);
    host.shutdown();
}

/// Two sessions of one matter each get a proxy, and one ending closes only its own.
#[test]
fn two_sessions_of_one_matter_close_their_proxies_independently() {
    let dir = Dir::new("egress_two_sessions", &[]);
    claude_then(&dir, &json!([{"name": "browser"}]), &[]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let first = host.create_thread(matter, CLAUDE);
    turn(&host, first);
    // The second session's agent exits after its turn; the first's keeps running.
    claude_then(&dir, &json!([{"name": "browser"}]), &[exit()]);
    let second = host.create_thread(matter, CLAUDE);
    turn(&host, second);
    let closed = closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the second session's proxy closed");
    assert_eq!(closed.thread, second);
    assert_eq!(closed.matter, matter);
    assert_eq!(calls(&log), ["open 1", "open 2", "removal", "close 2"]);
    host.shutdown();
    assert_eq!(
        calls(&log),
        [
            "open 1", "open 2", "removal", "close 2", "removal", "close 1"
        ]
    );
}

/// A panic in the removal, after the session's claims were made, still closes its proxy: the
/// run's guard closes it as the run drops.
#[test]
fn a_panic_in_the_removal_still_closes_the_proxy() {
    let dir = Dir::new("egress_panic", &[]);
    claude_then(&dir, &json!([{"name": "browser"}]), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_run_removal_hook(Arc::new(|_run: &Path| {
            panic!("a removal that fails (the test's)");
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the proxy closed after the panic");
    assert_eq!(calls(&log), ["open 1", "close 1"]);
    host.shutdown();
}
