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
use clauseo_protocol::{AgentId, StopReason, StreamId, ThreadId};
use serde_json::{Value, json};

use crate::egress::{Recording, calls, claude_then, wired};
use crate::profile_rig::{CLAUDE, Wired, turn};
use crate::support::{Dir, Running, wait};

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

/// A stop while a removal waits for a straggler ends that wait at once: shutdown answers well
/// before the cap, the proxy closes, and the folder stays for the next start's sweep.
#[cfg(target_os = "linux")]
#[test]
fn a_stop_mid_wait_frees_the_proxy_without_waiting_out_the_cap() {
    group::become_reaper();
    let dir = Dir::new("egress_stop_mid_wait", &[]);
    claude_then(
        &dir,
        &json!([{"name": "browser"}]),
        &[group::helper(), exit()],
    );
    let (proxy, log, closes) = Recording::new(false);
    let launcher = Wired::new(&dir);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        wired(config, proxy, removals).with_tree_wait(Duration::from_secs(600))
    });
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let helper = group::pid(&folder);
    group::until_zombie(helper);
    no_close_within(&closes, Duration::from_millis(500));
    let (stopped, stop) = channel();
    std::thread::spawn(move || {
        host.shutdown();
        let _gone = stopped.send(());
    });
    stop.recv_timeout(Duration::from_secs(30))
        .expect("the stop did not wait out the 600 s cap");
    assert_eq!(
        calls(&log),
        ["open 1", "close 1"],
        "closed at stop, not removed"
    );
    assert!(
        run_folder(&launcher).exists(),
        "the next start's sweep takes it"
    );
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
    // The first session ends too, before the host stops: a stop's removal waits only a short
    // grace for the tree's end, which a loaded Windows runner can outlast, and then closes
    // the proxy without the removal.
    let agent = AgentId::new(CLAUDE).unwrap();
    wait("stop_agents", host.handle.stop_agents(agent)).unwrap();
    let closed = closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the first session's proxy closed");
    assert_eq!(closed.thread, first);
    assert_eq!(
        calls(&log),
        [
            "open 1", "open 2", "removal", "close 2", "removal", "close 1"
        ]
    );
    host.shutdown();
    assert_eq!(
        calls(&log),
        [
            "open 1", "open 2", "removal", "close 2", "removal", "close 1"
        ]
    );
}

/// A panic in the removal after its wait completed (here in the removal hook) still frees the
/// session: the run's lease was marked done, so the proxy closes as the run drops.
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

/// A panic before the wait completed (here in the wait's own hook) frees nothing: the run's
/// lease holds the proxy until the host stops.
#[test]
fn a_panic_in_the_wait_holds_the_proxy_until_the_host_stops() {
    let dir = Dir::new("egress_wait_panic", &[]);
    claude_then(&dir, &json!([{"name": "browser"}]), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_tree_wait_hook(Arc::new(|_cdp| {
            panic!("a wait that fails (the test's)");
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    no_close_within(&closes, Duration::from_secs(1));
    assert_eq!(calls(&log), ["open 1"]);
    host.shutdown();
    assert_eq!(
        calls(&log),
        ["open 1", "close 1"],
        "freed at stop, never removed"
    );
}

/// A removal whose thread does not start drops its run on the host thread: the lease holds the
/// proxy (a push, no join), the host goes on answering, and the stop frees it.
#[test]
fn a_removal_thread_that_does_not_start_holds_the_proxy() {
    let dir = Dir::new("egress_no_thread", &[]);
    claude_then(&dir, &json!([{"name": "browser"}]), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_removal_thread_failing()
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    no_close_within(&closes, Duration::from_secs(1));
    // The host thread was not held up: it still answers.
    let other = host.create_thread(matter, CLAUDE);
    assert_ne!(other, thread);
    assert_eq!(calls(&log), ["open 1"]);
    host.shutdown();
    assert_eq!(calls(&log), ["open 1", "close 1"]);
}

/// How long a launch waits for a slot a running removal may free (`launch_checks::browser`).
const SLOT_WAIT: Duration = Duration::from_secs(3);

/// Runs a turn on `thread` whose launch waits for a browser slot: once the launch has armed
/// its wait on the host clock, `then` (which ends the wait), and the turn's end.
fn turn_after_slot_wait(host: &Running, thread: ThreadId, then: impl FnOnce()) {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    let armed = (0..256).any(|_| wait("a timer armed", host.clock.next_armed()) == SLOT_WAIT);
    assert!(armed, "the launch did not wait for a slot");
    then();
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
}

/// A session started while every slot is taken and an ended session's removal still runs
/// waits for that removal and gets the slot it frees, rather than `TooManyBrowsers`. The
/// removal is held at its start until the next launch waits (on Windows the removal's look at
/// Chrome's port alone takes longer than a launch). It ends right after the launch registered
/// its wait, and the host clock never moves: the removal's end alone ends the wait.
#[test]
fn a_launch_gets_the_slot_an_ending_session_frees() {
    use clauseo_protocol::{ClauseoTool, ToolAvailability};

    let dir = Dir::new("egress_slot_freed", &[]);
    claude_then(&dir, &json!("{{any}}"), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let (go, gate) = channel::<()>();
    let (began, removing) = channel::<()>();
    let gate = Mutex::new(Some((began, gate)));
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_tree_wait_hook(Arc::new(move |_cdp| {
            // Only the first removal is held.
            if let Some((began, gate)) = gate.lock().unwrap().take() {
                began.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(30)).unwrap();
            }
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let ending = host.create_thread(matter, CLAUDE);
    turn(&host, ending);
    claude_then(&dir, &json!("{{any}}"), &[]);
    let running = host.create_thread(matter, CLAUDE);
    turn(&host, running);
    removing
        .recv_timeout(Duration::from_secs(30))
        .expect("the ended session's removal began");
    let next = host.create_thread(matter, CLAUDE);
    turn_after_slot_wait(&host, next, || go.send(()).unwrap());
    let tools = crate::profile_rig::first_frame_tools(&host, next);
    let browser = tools
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser);
    assert_eq!(
        browser.map(|state| &state.availability),
        Some(&ToolAvailability::Available)
    );
    closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the ended session was freed");
    host.shutdown();
}

/// A session's browser slot stays taken while its removal waits for Chrome: with the machine's
/// two slots, one waiting and one running, a third session gets `TooManyBrowsers`; once the
/// waiting one is freed, a fourth gets the browser.
#[test]
fn the_browser_slot_stays_taken_during_the_wait() {
    use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolsUnavailableReason};

    let browser = |host: &crate::support::Running, thread| {
        crate::profile_rig::first_frame_tools(host, thread)
            .into_iter()
            .find(|state| state.tool == ClauseoTool::Browser)
            .expect("the header shows the browser")
            .availability
    };
    let dir = Dir::new("egress_slot", &[]);
    // Each session takes whatever servers it gets; the first exits after its turn.
    claude_then(&dir, &json!("{{any}}"), &[exit()]);
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let (bound, listener) = channel::<TcpListener>();
    let bound = Mutex::new(bound);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals).with_tree_wait_hook(Arc::new(move |cdp| {
            if let Some(port) = cdp {
                let chrome = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
                let _sent = bound.lock().unwrap().send(chrome);
            }
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let waiting = host.create_thread(matter, CLAUDE);
    turn(&host, waiting);
    let chrome = listener
        .recv_timeout(Duration::from_secs(30))
        .expect("the first session's removal began its wait");
    claude_then(&dir, &json!("{{any}}"), &[]);
    let running = host.create_thread(matter, CLAUDE);
    turn(&host, running);
    assert_eq!(browser(&host, running), ToolAvailability::Available);
    let third = host.create_thread(matter, CLAUDE);
    // Its launch waits for the waiting session's removal, which runs on, until the slot wait
    // runs out.
    turn_after_slot_wait(&host, third, || host.clock.advance(SLOT_WAIT));
    assert_eq!(
        browser(&host, third),
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::TooManyBrowsers
        },
        "the waiting session still holds its slot"
    );
    let opens = || {
        calls(&log)
            .iter()
            .filter(|call| call.starts_with("open"))
            .count()
    };
    assert_eq!(
        opens(),
        2,
        "a launch left without the browser opens no proxy"
    );
    drop(chrome);
    closes
        .recv_timeout(Duration::from_secs(30))
        .expect("the first session was freed");
    // The freed run's slot goes back on the host thread just after the close.
    let (_keep, never) = channel::<()>();
    let _timed_out = never.recv_timeout(Duration::from_millis(500));
    host.settle();
    let fourth = host.create_thread(matter, CLAUDE);
    turn(&host, fourth);
    assert_eq!(browser(&host, fourth), ToolAvailability::Available);
    host.shutdown();
}
