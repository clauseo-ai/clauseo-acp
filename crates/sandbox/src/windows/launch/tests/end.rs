//! The outer runner's start (its spec, options and denial pipe), what its exit means to the
//! host, and the session's end: once, whoever asks first, after the pool user's processes have
//! truly exited.

use super::fake::World;
use super::feeding::Denials;
use super::pipe::{PIPE, install};
use super::*;
use crate::NoSandbox;
use crate::windows::runner::{HOST_ENDED, RUN_EXIT_BASE, RunExit};

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
    world.outer.spawn_starts();
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
            ["create_pipe", "write_launch", "spawn_outer", "watch_outer"]
        );
        assert_eq!(world.outer.denial_pipe().as_deref(), Some(PIPE));
        let outer = world.outer.spawned().unwrap();
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
    assert!(world.outer.spawned().is_none());
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
    assert_eq!(
        world.calls(),
        [&["remove_launch", "wait_exited 2"][..], &PASS].concat()
    );
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
    world.outer.linger(1);
    let before = world.now();
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let want = [
        &["remove_launch", "wait_exited 2", "stop", "wait_exited 0"][..],
        &PASS,
    ]
    .concat();
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
    assert_eq!(
        world.calls(),
        [&["remove_launch", "wait_exited 2"][..], &PASS].concat()
    );
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
    world.outer.linger(2);
    let before = world.now();
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let want = [
        &["remove_launch", "wait_exited 2", "stop", "wait_exited 2"][..],
        &PASS,
    ]
    .concat();
    assert_eq!(world.calls(), want);
    assert_eq!(world.now().saturating_sub(before), EXIT_WAIT * 2);
    let other = World::new();
    let launchers = super::launchers(&other);
    let launcher = launched(&launchers, &other, &dir, SpawnOptions::new());
    start(&*launcher, &other, &dir);
    other.calls();
    other.fail_at("wait_exited");
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(
        other.calls(),
        [&["remove_launch", "wait_exited 2"][..], &PASS].concat()
    );
}

/// The launch file goes once the outer has connected to the denial pipe (it has read the file
/// by then), from the feed's thread; the session's end then has none left to delete. With no
/// connection, the end deletes it (the tests above).
#[test]
fn the_launch_file_goes_once_the_outer_connects() {
    let world = World::new();
    let dir = TempDir::new("launch-file-connect");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    world.outer.pipe_connects();
    start(&*launcher, &world, &dir);
    world.wait_for("remove_launch");
    assert_eq!(
        world.calls(),
        [
            "create_pipe",
            "write_launch",
            "spawn_outer",
            "watch_outer",
            "remove_launch"
        ]
    );
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(world.calls(), [&["wait_exited 2"][..], &PASS].concat());
}

/// A session whose end began before its outer started gets no agent: refused before the
/// spawn, and, when the end came during the spawn, the outer is ended and the launch fails.
/// Either way the pass ran once, from `after_session`, and the launch file is gone.
#[test]
fn an_end_before_the_start_fails_the_launch() {
    let world = World::new();
    let dir = TempDir::new("launch-end-first");
    let launchers = launchers(&world);
    let policy = policy_at(&dir, "matter");
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    launchers.after_session(MATTER, "s1", &policy);
    assert_eq!(world.calls(), PASS);
    world.outer.spawn_starts();
    assert!(launcher.launch(&spec(&dir)).is_err());
    assert_eq!(
        world.calls(),
        Vec::<String>::new(),
        "nothing written or started"
    );
}

/// An end that comes while the outer is starting moves nothing: no repair, no set-aside, and
/// the slot and the matter stay held. The launcher opens the outer (before its handle can
/// drop), ends it, and only once `end_outer` has seen its job empty runs the end's pass, which
/// waits for the pool user's processes, then repairs, sets aside and frees. If the outer can't
/// be ended within its bound, nothing moves and the matter is stuck.
#[test]
fn an_end_during_the_spawn_moves_nothing_until_the_outer_is_gone() {
    for end_fails in [false, true] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-end-race-{end_fails}"));
        let launchers = launchers(&world);
        let policy = policy_at(&dir, "matter");
        let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
        world.outer.spawn_starts();
        if end_fails {
            world.fail_at("end_outer");
        }
        let held = world.hold("spawn_outer");
        std::thread::scope(|scope| {
            let starting = scope.spawn(|| launcher.launch(&spec(&dir)).is_err());
            held.reached();
            let moved = launchers.after_session(MATTER, "s1", &policy);
            assert_eq!(moved, Vec::<SetAside>::new());
            let busy = launchers.reserve(MATTER, "s2", &[]);
            assert!(matches!(busy, Err(Error::MatterBusy)), "held: {busy:?}");
            assert!(!world.claimed_slots().is_empty(), "the slot stays claimed");
            held.go();
            assert!(starting.join().unwrap(), "the launch failed");
        });
        let calls = world.calls();
        let at = calls.iter().position(|call| call == "watch_outer").unwrap();
        assert_eq!(
            calls[..at],
            ["create_pipe", "write_launch", "spawn_outer"],
            "nothing moved while the outer was starting"
        );
        // Opened first; the handle dropped (the outer ended) before the file and `end_outer`.
        let head = ["watch_outer", "remove_launch", "end_outer"];
        let next = launchers.reserve(MATTER, "s2", &[]);
        if end_fails {
            assert_eq!(calls[at..], head, "no repair, no set-aside: quarantined");
            assert!(matches!(next, Err(Error::MatterStuck)), "{next:?}");
        } else {
            let pass = [&head[..], &["wait_exited 2"], &PASS].concat();
            assert_eq!(calls[at..], pass);
            assert!(next.is_ok(), "{next:?}");
        }
        drop(launcher);
        assert!(launchers.wait_ends(WAIT));
    }
}

/// A launch file whose removal fails is noted and tried again at the session's end; one that
/// still won't go is removed by the next start's stale sweep.
#[test]
fn a_launch_file_that_wont_go_is_retried() {
    let world = World::new();
    let dir = TempDir::new("launch-file-retry");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    world.outer.pipe_connects();
    world.fail_at("remove_launch");
    start(&*launcher, &world, &dir);
    world.wait_for("remove_launch");
    assert_eq!(world.outer.files().len(), 1, "the feed's removal failed");
    world.fail_at("none");
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert!(world.outer.files().is_empty(), "retried at the end");
    drop(launcher);
    *world.outer.pipe_connects.lock().unwrap() = false;
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    world.fail_at("remove_launch");
    start(&*launcher, &world, &dir);
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(world.outer.files().len(), 1, "the end's removal failed too");
    world.fail_at("none");
    // The next start (another data folder, so the first launchers' lock doesn't matter).
    let other = TempDir::new("launch-file-retry-next");
    let _next = Launchers::with_steps(FakeSteps::elsewhere(&world, other.path())).unwrap();
    assert!(
        world.outer.files().is_empty(),
        "the next start's sweep removed it"
    );
    drop((launcher, launchers));
}

/// A pass that panics on the dropped launcher's thread still counts as done: `wait_ends`
/// doesn't wait out its timeout, and the matter is freed by the pass's own guards.
#[test]
fn a_panicking_end_still_counts_as_done() {
    let world = World::new();
    let dir = TempDir::new("launch-end-panics");
    let launchers = launchers(&world);
    let launcher = launched(&launchers, &world, &dir, SpawnOptions::new());
    start(&*launcher, &world, &dir);
    world.panic_at("repair");
    drop(launcher);
    assert!(launchers.wait_ends(WAIT));
    assert!(launchers.registry.wait_free(MATTER, WAIT));
}
