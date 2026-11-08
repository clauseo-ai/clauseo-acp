//! R17 PR-E: a forgotten matter's sandbox forget is owed until it succeeds. The host records it
//! in `<data>/protection/owed_forgets.json` before the walk, drops it once the walk succeeds,
//! keeps it when the walk fails or the quit's bound passes first, and replays it at the next
//! start; the walk waits for the matter's agents to be gone.

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{CommandOutcome, HostCommand, MatterId};
use clauseo_sandbox::{SetupError, SetupState};

use crate::protection::dir;
use crate::protection_rig::{FakeSetup, Gated, Plan, b, rig};
use crate::support::{Dir, Running, wait};

/// The stop's bound on a forget still running.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// The matters the owed list on disk names; none when there is no file.
fn owed(dir: &Dir) -> Vec<MatterId> {
    let path = dir.data_dir().join("protection").join("owed_forgets.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("reading the owed list: {error}"),
    };
    let disk: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(disk["v"], 1, "{disk}");
    serde_json::from_value(disk["matters"].clone()).unwrap()
}

/// Waits until the owed list on disk is `expected`.
fn until_owed(dir: &Dir, expected: &[MatterId]) {
    wait("the owed list", async {
        while owed(dir) != expected {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// Waits until the set-up was asked to forget `count` matters, and returns them.
fn until_forgotten(setup: &FakeSetup, count: usize) -> Vec<(MatterId, String)> {
    wait("the sandbox's forget", async {
        loop {
            let forgotten = setup.forgotten.lock().unwrap().clone();
            if forgotten.len() >= count {
                break forgotten;
            }
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    })
}

/// A host of run `run` on `dir` with a tier-B seam whose forgets end as `forgets` says.
fn start(
    dir: &Dir,
    run: u16,
    forgets: Vec<Result<(), SetupError>>,
    gated: Gated,
) -> (Running, crate::protection_rig::Rig) {
    let mut plan = Plan::new(vec![b()], vec![SetupState::Ready]);
    plan.forgets = forgets;
    plan.gated = gated;
    let rig = rig(plan);
    let sandbox = rig.sandbox.clone();
    let host = dir.start_with(run, move |config| config.with_sandbox(sandbox));
    (host, rig)
}

/// A matter made and forgotten on `host`.
fn forget_one(dir: &Dir, host: &Running) -> MatterId {
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    assert_eq!(
        host.dispatch(HostCommand::ForgetMatter { matter }),
        Ok(CommandOutcome::Done)
    );
    matter
}

/// The next start replays what `dir` owes for `matter`: walked once, then nothing owed.
fn replayed(dir: &Dir, run: u16, matter: MatterId) {
    let (host, rig) = start(dir, run, vec![Ok(())], Gated::No);
    let forgotten = until_forgotten(&rig.setup, 1);
    assert_eq!(
        forgotten,
        vec![(matter, "clauseo-sandbox-forget".to_owned())]
    );
    until_owed(dir, &[]);
    host.shutdown();
    assert_eq!(owed(dir), Vec::<MatterId>::new());
}

/// A forget that succeeds was owed while it ran and leaves nothing owed.
#[test]
fn a_forget_that_succeeds_leaves_nothing_owed() {
    let dir = dir("protection_forget_ok");
    let (host, rig) = start(&dir, 0, vec![Ok(())], Gated::No);
    let matter = forget_one(&dir, &host);
    assert_eq!(until_forgotten(&rig.setup, 1)[0].0, matter);
    until_owed(&dir, &[]);
    host.shutdown();
    assert_eq!(owed(&dir), Vec::<MatterId>::new());
}

/// A forget that fails stays owed past the stop; the next start replays it, then clears it.
#[test]
fn a_failed_forget_stays_owed_and_the_next_start_replays_it() {
    let dir = dir("protection_forget_failed");
    let failed = vec![Err(SetupError::Failed(
        "the ledger did not read".to_owned(),
    ))];
    let (host, rig) = start(&dir, 0, failed, Gated::No);
    let matter = forget_one(&dir, &host);
    assert_eq!(until_forgotten(&rig.setup, 1)[0].0, matter);
    host.shutdown();
    assert_eq!(owed(&dir), vec![matter]);
    replayed(&dir, 1, matter);
}

/// A quit while the forget runs waits for it 2 s on the host clock, then stops with the forget
/// still owed; the next start replays it.
#[test]
fn a_quit_mid_forget_leaves_it_owed_and_the_next_start_replays_it() {
    let dir = dir("protection_forget_quit");
    let (host, mut rig) = start(&dir, 0, vec![Ok(())], Gated::Forget);
    let gate = rig.gate.take().unwrap();
    let matter = forget_one(&dir, &host);
    gate.started.recv().unwrap();
    // Recorded before the walk began.
    assert_eq!(owed(&dir), vec![matter]);
    let clock = host.clock.clone();
    let stopping = std::thread::spawn(move || host.shutdown());
    wait("the stop's wait for the forget", async {
        while clock.next_armed().await != STOP_WAIT {}
    });
    assert!(
        !stopping.is_finished(),
        "the stop did not wait for the forget"
    );
    clock.advance(STOP_WAIT);
    stopping.join().unwrap();
    assert_eq!(owed(&dir), vec![matter]);
    replayed(&dir, 1, matter);
    // The first run's walk was given up on: released now, it ends and writes nothing.
    drop(gate);
}

/// The forget waits while the matter's agent is still being stopped (it ignores its stdin's
/// close until the ladder's SIGTERM), and runs once it has ended. Unix only, as the ladder's
/// SIGTERM step is.
#[cfg(unix)]
#[test]
fn the_forget_waits_for_the_matter_s_agent_to_end() {
    use clauseo_protocol::{StopReason, StreamId};
    use clauseo_sandbox::NoSandbox;

    use crate::protection_rig::c;

    use crate::support::{FAKE, Scenario, matter_with_thread};

    /// The ladder's wait after it closes the agent's stdin, before SIGTERM.
    const STDIN_WAIT: Duration = Duration::from_secs(2);

    let dir = Dir::new(
        "protection_forget_agent",
        &[(FAKE, Scenario::Local("idle-stop-slow-exit-then-load"))],
    );
    // Tier C, its set-up not done, and not required: the agent runs unjailed, and the set-up
    // is there to forget.
    dir.set_settings(
        &[(FAKE, Scenario::Local("idle-stop-slow-exit-then-load"))],
        serde_json::json!({"sandbox": {"required": false, "allow_unprotected_auto": true}}),
    );
    let rig = rig(Plan::new(
        vec![c(NoSandbox::NeedsSetUp)],
        vec![SetupState::Needed],
    ));
    let sandbox = rig.sandbox.clone();
    let host = dir.start_with(0, move |config| config.with_sandbox(sandbox));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    assert_eq!(
        host.ok(HostCommand::ForgetMatter { matter }),
        CommandOutcome::Done
    );
    // The agent is being stopped: its stdin is closed, and it still runs.
    host.wait_armed(STDIN_WAIT);
    until_owed(&dir, &[matter]);
    host.settle();
    assert!(
        rig.setup.forgotten.lock().unwrap().is_empty(),
        "the forget ran while the matter's agent was live"
    );
    // The clock moves on through the rest of the ladder (SIGTERM, then the kill) until the
    // agent has ended and the forget runs: under load the grace seen above may not be the
    // last timer the ladder arms.
    let clock = host.clock.clone();
    let forgotten = wait("the forget once the agent ended", async {
        loop {
            let forgotten = rig.setup.forgotten.lock().unwrap().clone();
            if !forgotten.is_empty() {
                break forgotten;
            }
            clock.advance(STDIN_WAIT);
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    assert_eq!(forgotten[0].0, matter);
    until_owed(&dir, &[]);
    host.shutdown();
}

/// The owed list's path, as the host names it.
#[test]
fn the_owed_list_lives_in_the_data_folder_s_protection_folder() {
    let dir = dir("protection_forget_path");
    let (host, rig) = start(&dir, 0, vec![Err(SetupError::Refused)], Gated::No);
    let matter = forget_one(&dir, &host);
    until_forgotten(&rig.setup, 1);
    host.shutdown();
    let path: &Path = &dir.data_dir().join("protection").join("owed_forgets.json");
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text, format!("{{\"v\":1,\"matters\":[\"{matter}\"]}}"));
}
