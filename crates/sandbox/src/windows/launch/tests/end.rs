//! The outer runner's start (its spec, options and denial feed), what its exit means to the
//! host, and the session's end: once, whoever asks first, after the pool user's processes have
//! truly exited.

use std::sync::Mutex;

use super::fake::{World, install};
use super::pipe::{FakePipe, PIPE};
use super::*;
use crate::windows::runner::{HOST_ENDED, RUN_EXIT_BASE, RunExit};
use crate::{Denial, DenialSink, NoSandbox, SetAsideWhat};

/// Records the denials it gets, in order.
#[derive(Default)]
struct Denials(Mutex<Vec<Denial>>);

impl DenialSink for Denials {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

/// A sink that panics on every denial.
struct Panics;

impl DenialSink for Panics {
    fn denied(&self, _denial: Denial) {
        panic!("a bad sink");
    }
}

/// The calls of the after-session pass once the pool user's processes are gone.
const PASS: [&str; 7] = [
    "session_cursor",
    "repair",
    "end_reads",
    "set_aside_after",
    "write_clean_close cursor=true repaired=true passed=true",
    "end_run 1",
    "release_slot",
];

/// The launcher for `s1` of `MATTER`, launched under the host's `opts`; the calls taken.
fn launched(
    launchers: &Launchers<FakeSteps>,
    world: &World,
    dir: &TempDir,
    opts: SpawnOptions,
) -> Arc<dyn clauseo_process::Launcher> {
    let reservation = prepared(launchers, world, dir, "s1");
    let policy = policy_at(dir, "matter");
    let (launcher, _) = launchers
        .launch(
            reservation,
            policy,
            MATTER,
            "s1",
            opts,
            Arc::new(Denials::default()),
        )
        .unwrap();
    world.calls();
    launcher
}

/// Starts the agent through `launcher`: the fake starts a real process that exits at once,
/// and the outer's two processes run as the pool user until a wait for them. Its handle is
/// dropped (the host's, once the tree is gone).
fn start(launcher: &dyn clauseo_process::Launcher, world: &World, dir: &TempDir) {
    world.spawn_starts();
    drop(launcher.launch(&spec(dir)).unwrap());
}

fn spec(dir: &TempDir) -> clauseo_process::LaunchSpec {
    clauseo_process::LaunchSpec::new(
        dir.file("bin/agent.exe", ""),
        Vec::new(),
        std::collections::BTreeMap::new(),
        dir.dir("matter"),
    )
    .unwrap()
}

/// One record of the stand-in format.
fn record(path: &str, to: &str, what: &str) -> Vec<u8> {
    format!(r#"{{"kind":"set_aside","path":"{path}","to":"{to}","what":"{what}"}}"#).into_bytes()
}

/// The outer, as the fake pipe opens it.
const OUTER: Peer = Peer {
    pid: 4242,
    started: 133_000_000_000_000_000,
};

/// A pipe the outer opened, holding `chunks`.
fn from_outer(chunks: Vec<Vec<u8>>) -> FakePipe {
    FakePipe {
        outer: Some(OUTER),
        client: Some(OUTER),
        chunks: chunks.into(),
        read_fails: false,
        exits_after: None,
    }
}

/// The outer's records reach the sink in order, however the reads split them; a line that
/// isn't a record (another kind, bad JSON, the old stderr line, one over the cap, one cut off
/// at the end) is dropped, and a sink that panics doesn't end the feed.
#[test]
fn denial_records_reach_the_sink_in_order_and_others_dont() {
    let git = record(".git", r"set-aside\\1\\.git", "git_repository");
    let link = record("link", r"set-aside\\2\\link", "link_out");
    let mut stream = Vec::new();
    for line in [
        git.clone(),
        br#"{"kind":"notice","words":"hello"}"#.to_vec(),
        b"not json".to_vec(),
        format!(
            "clauseo-sandbox-run: set aside\tfoo\tbar\t{}",
            SetAsideWhat::LinkOut.why()
        )
        .into_bytes(),
        record(&"x".repeat(70 * 1024), "y", "link_out"),
        link,
    ] {
        stream.extend_from_slice(&line);
        stream.push(b'\n');
    }
    stream.extend_from_slice(&git[..10]);
    // Reads of 7 bytes, so records and the line over the cap come in pieces.
    let chunks = stream.chunks(7).map(<[u8]>::to_vec).collect();
    let sink = Denials::default();
    let fed = feed::feed(from_outer(chunks), OUTER.pid, &sink);
    assert_eq!(
        fed,
        feed::Fed::Ended {
            passed: 2,
            dropped: 5
        }
    );
    let set_aside = |path: &str, to: &str, what| Denial::SetAside {
        path: path.into(),
        to: to.into(),
        what,
    };
    assert_eq!(
        *sink.0.lock().unwrap(),
        [
            set_aside(".git", r"set-aside\1\.git", SetAsideWhat::GitRepository),
            set_aside("link", r"set-aside\2\link", SetAsideWhat::LinkOut),
        ]
    );
    let mut ends_badly = from_outer(vec![git.clone(), b"\n".to_vec()]);
    ends_badly.read_fails = true;
    assert_eq!(
        feed::feed(ends_badly, OUTER.pid, &Panics),
        feed::Fed::Ended {
            passed: 1,
            dropped: 0
        }
    );
}

/// Only the outer is read: a client with another pid, or the outer's pid with another
/// creation time (a reused id), is refused before a byte is read. No client, or an outer that
/// can't be opened, feeds nothing either.
#[test]
fn the_pipe_reads_the_outer_alone() {
    let lines = vec![[record("a", "b", "link_out"), b"\n".to_vec()].concat()];
    for client in [
        Peer { pid: 7, ..OUTER },
        Peer {
            started: OUTER.started + 1,
            ..OUTER
        },
    ] {
        let sink = Denials::default();
        let pipe = FakePipe {
            client: Some(client),
            ..from_outer(lines.clone())
        };
        assert_eq!(feed::feed(pipe, OUTER.pid, &sink), feed::Fed::Refused);
        assert_eq!(*sink.0.lock().unwrap(), []);
    }
    let sink = Denials::default();
    let nobody = FakePipe {
        client: None,
        ..from_outer(lines.clone())
    };
    assert_eq!(feed::feed(nobody, OUTER.pid, &sink), feed::Fed::NoClient);
    let gone = from_outer(lines);
    assert_eq!(feed::feed(gone, OUTER.pid + 1, &sink), feed::Fed::Failed);
    assert_eq!(*sink.0.lock().unwrap(), []);
}

/// The outer's exit ends the feed: a record that comes after it, and one it cut off, is never
/// delivered, though the pipe still holds them.
#[test]
fn records_after_the_outers_exit_are_dropped() {
    let line = |path: &str| [record(path, "aside", "link_out"), b"\n".to_vec()].concat();
    let cut = record("cut", "aside", "link_out");
    let chunks = vec![
        line("before"),
        cut[..8].to_vec(),
        cut[8..].to_vec(),
        line("after"),
    ];
    let sink = Denials::default();
    let pipe = FakePipe {
        exits_after: Some(2),
        ..from_outer(chunks)
    };
    assert_eq!(
        feed::feed(pipe, OUTER.pid, &sink),
        feed::Fed::Exited {
            passed: 1,
            dropped: 1
        }
    );
    assert_eq!(
        *sink.0.lock().unwrap(),
        [Denial::SetAside {
            path: "before".into(),
            to: "aside".into(),
            what: SetAsideWhat::LinkOut,
        }]
    );
}

/// The outer: the runner with `--launch <file>` in its folder, only `OUTER_ENV`'s variables
/// that are set, `clear_env` and a 5 s stdin grace (a grace the host set stays). The launch
/// file names the denial pipe made first, and stays once the outer started.
#[test]
fn the_outer_starts_minimal_with_its_pipe_and_a_5s_grace() {
    let cases = [
        (SpawnOptions::new(), OUTER_STDIN_GRACE),
        (
            SpawnOptions::new().stdin_grace(Duration::from_secs(2)),
            Duration::from_secs(2),
        ),
    ];
    assert_eq!(OUTER_STDIN_GRACE, Duration::from_secs(5));
    for (opts, grace) in cases {
        let world = World::new();
        let dir = TempDir::new("launch-outer");
        let launchers = launchers(&world);
        let launcher = launched(&launchers, &world, &dir, opts);
        start(&*launcher, &world, &dir);
        assert_eq!(
            world.calls(),
            ["create_pipe", "write_launch", "spawn_outer"]
        );
        assert_eq!(world.denial_pipe().as_deref(), Some(PIPE));
        let outer = world.spawned().unwrap();
        assert_eq!(outer.program, install().join("clauseo-sandbox-run.exe"));
        assert_eq!(outer.args, ["--launch", "run/0123456789abcdef.json"]);
        assert_eq!(outer.cwd, install());
        let names: Vec<_> = outer
            .env
            .keys()
            .map(|name| name.to_str().unwrap())
            .collect();
        assert_eq!(names, ["SystemRoot", "TEMP", "windir"]);
        assert_eq!(outer.grace, Some(grace));
        assert!(outer.clears_env);
    }
}

/// A pipe that can't be made fails the launch before anything is written or started: the
/// session is undone, and no pass runs, not even when the launcher drops.
#[test]
fn a_pipe_that_cant_be_made_fails_the_launch() {
    let world = World::new();
    let dir = TempDir::new("launch-no-pipe");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    world.fail_at("create_pipe");
    assert!(launcher.launch(&spec(&dir)).is_err());
    assert_eq!(
        world.calls(),
        ["create_pipe", "end_run 1", UNDO[0], UNDO[1]]
    );
    assert!(world.spawned().is_none());
    drop(launcher);
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), Vec::<String>::new());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// The outer's exit status for `code`: `ExitStatus::code()`, the Windows code's bits.
fn exit(code: u32) -> Option<i32> {
    i32::try_from(code)
        .ok()
        .or(Some(i32::from_ne_bytes(code.to_ne_bytes())))
}

/// What each exit means to the host: the agent's own code, the host's own end, the escapee's
/// stop (over any code), a set-up failure's new detection, and a failed launch for any other
/// runner failure or a code this build doesn't know.
#[test]
fn each_run_outcome_maps_to_its_host_meaning() {
    for code in [0, 3, 0xC000_0005] {
        assert!(
            matches!(RunEnd::from_exit(exit(code)), RunEnd::Exited(c) if c == code),
            "{code:#x}"
        );
    }
    assert!(matches!(
        RunEnd::from_exit(exit(HOST_ENDED)),
        RunEnd::HostEnded
    ));
    assert!(matches!(
        RunEnd::from_exit(exit(RunExit::Escapee.code())),
        RunEnd::LeftSafeWorkspace
    ));
    let redetect = RunEnd::from_exit(exit(RunExit::LogonRight.code()));
    assert!(
        matches!(
            &redetect,
            RunEnd::Redetect(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight))
        ),
        "{redetect:?}"
    );
    let every = (0..=0xff).filter_map(|n| RunExit::from_code(RUN_EXIT_BASE + n));
    for each in every {
        let end = RunEnd::from_exit(exit(each.code()));
        match (each, each.no_sandbox()) {
            (RunExit::Escapee, _) => assert!(matches!(end, RunEnd::LeftSafeWorkspace)),
            (_, Some(cause)) => assert!(matches!(&end, RunEnd::Redetect(c) if *c == cause)),
            (_, None) => assert!(matches!(&end, RunEnd::Failed(Error::Launch(_))), "{end:?}"),
        }
    }
    for failed in [None, exit(RunExit::Spawn.code())] {
        let RunEnd::Failed(error) = RunEnd::from_exit(failed) else {
            panic!("{failed:?} didn't fail the launch");
        };
        assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    }
}

/// A normal end: the host's `after_session` runs the pass once, after the pool user's
/// processes have exited; neither a second call nor the launcher's drop runs it again; the
/// matter is busy until then and reserved again after.
#[test]
fn a_normal_end_runs_the_pass_once() {
    let world = World::new();
    let dir = TempDir::new("launch-end-normal");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    start(&*launcher, &world, &dir);
    world.calls();
    assert!(matches!(
        launchers.reserve(MATTER, "s2", &[]),
        Err(Error::MatterBusy)
    ));
    let policy = policy_at(&dir, "matter");
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        [fake::moved("AGENTS.md")]
    );
    assert_eq!(world.calls(), [&["wait_exited 2"][..], &PASS].concat());
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        Vec::<SetAside>::new()
    );
    drop(launcher);
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), Vec::<String>::new());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A cancel during the preparation ends in the reservation's undo, once: no pass (nothing was
/// launched), and the host's `after_session` after it does nothing. The matter is free once
/// the host drops the reservation.
#[test]
fn a_cancel_during_prepare_ends_once() {
    let world = World::new();
    let dir = TempDir::new("launch-end-cancel-prepare");
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let held = world.hold("apply");
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    held.reached();
    assert!(matches!(
        launchers.reserve(MATTER, "s2", &[]),
        Err(Error::MatterBusy)
    ));
    preparing.cancel();
    held.go();
    assert!(matches!(preparing.wait(WAIT), Some(Err(Error::Cancelled))));
    let calls = world.calls();
    assert_eq!(calls[calls.len() - 3..], ["end_run 1", UNDO[0], UNDO[1]]);
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter")),
        Vec::<SetAside>::new()
    );
    drop(reservation);
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), Vec::<String>::new());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A cancel during the run: the host's ladder ends the outer's job, but its processes still
/// tear down past the first wait. The pass waits, has the stop task end them, waits again,
/// then runs once; the drop after it does nothing.
#[test]
fn a_cancel_during_the_run_ends_once_after_the_processes_exit() {
    let world = World::new();
    let dir = TempDir::new("launch-end-cancel-run");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    start(&*launcher, &world, &dir);
    world.calls();
    world.linger(1);
    let before = world.now();
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let want = [&["wait_exited 2", "stop", "wait_exited 0"][..], &PASS].concat();
    assert_eq!(world.calls(), want);
    assert_eq!(
        world.now().saturating_sub(before),
        EXIT_WAIT,
        "the first wait ran out"
    );
    drop(launcher);
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), Vec::<String>::new());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A launcher dropped with no `after_session` (an early drop, the host's quit) runs the pass
/// itself, once, off the dropping thread; the host's call after it does nothing. The matter is
/// busy until the pass has finished.
#[test]
fn a_dropped_launcher_runs_the_pass_once() {
    let world = World::new();
    let dir = TempDir::new("launch-end-drop");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    start(&*launcher, &world, &dir);
    world.calls();
    let held = world.hold("set_aside_after");
    drop(launcher);
    held.reached();
    assert!(matches!(
        launchers.reserve(MATTER, "s2", &[]),
        Err(Error::MatterBusy)
    ));
    assert!(!launchers.wait_ends(Duration::ZERO));
    held.go();
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), [&["wait_exited 2"][..], &PASS].concat());
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter")),
        Vec::<SetAside>::new()
    );
    assert_eq!(world.calls(), Vec::<String>::new());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A launcher dropped before it started the outer: its claim undoes the session (C3), and its
/// guard, never armed, runs no pass; `after_session` after it finds nothing.
#[test]
fn a_launcher_dropped_unused_is_undone_not_ended() {
    let world = World::new();
    let dir = TempDir::new("launch-end-unused");
    let launchers = launchers(&world);
    drop(launched(&launchers, &world, &dir, SpawnOptions::new()));
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert!(launchers.wait_ends(WAIT));
    assert_eq!(world.calls(), ["end_run 1", UNDO[0], UNDO[1]]);
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter")),
        Vec::<SetAside>::new()
    );
    assert_eq!(world.calls(), Vec::<String>::new());
}

/// Processes that survive the stop task too: the pass waits the bound twice, then goes on (the
/// next preparation's leftover check meets them); one that can't wait goes on too.
#[test]
fn the_end_waits_for_processes_to_truly_exit() {
    let world = World::new();
    let dir = TempDir::new("launch-end-stubborn");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    start(&*launcher, &world, &dir);
    world.calls();
    world.leftovers(2, true);
    world.linger(2);
    let before = world.now();
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let want = [&["wait_exited 2", "stop", "wait_exited 2"][..], &PASS].concat();
    assert_eq!(world.calls(), want);
    assert_eq!(world.now().saturating_sub(before), EXIT_WAIT * 2);
    let other = World::new();
    let launchers = super::launchers(&other);
    let launcher = launched(&launchers, &other, &dir, SpawnOptions::new());
    start(&*launcher, &other, &dir);
    other.calls();
    other.fail_at("wait_exited");
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(other.calls(), [&["wait_exited 2"][..], &PASS].concat());
}
