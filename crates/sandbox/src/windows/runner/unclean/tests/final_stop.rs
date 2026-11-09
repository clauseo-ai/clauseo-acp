//! Stop, sweep, stop: a marker past its first stop is clean only once a stop that reserved its
//! number after the sweep completed has completed too. The world here runs the pool tasks at
//! once: a stop kills every pool process and completes, and a sweep deletes the pool's tasks and
//! completes, but a pool task's trigger can start a process before the sweep deletes it.

use super::*;

/// The fake's markers and numbers, with the pool's processes and the tasks run at once.
struct World {
    fake: Fake,
    /// A pool task the escapee registered: its trigger fires once before the sweep deletes it.
    trigger: bool,
    /// A pool process is alive: one the trigger started after the first stop's snapshot.
    alive: bool,
    /// `StopNext` can't be read.
    stop_next_fails: bool,
}

impl World {
    fn new(stop: u64) -> World {
        let mut fake = Fake::new();
        // The escapee's session: its marker read `StopNext` when it ended.
        fake.stop_next = stop;
        fake.marker("a1", Some(POOL_A), Some(MarkerState::AwaitingStop { stop }));
        World {
            fake,
            trigger: true,
            alive: false,
            stop_next_fails: false,
        }
    }
}

impl Markers for World {
    fn sessions(&mut self) -> io::Result<Vec<UncleanSession>> {
        self.fake.sessions()
    }
    fn sequences(&mut self) -> Result<Sequences, String> {
        self.fake.sequences()
    }
    fn next_number(&mut self, name: &'static str) -> Result<u64, String> {
        if name == STOP_NEXT && self.stop_next_fails {
            return Err("RegGetValueW failed (5)".to_owned());
        }
        self.fake.next_number(name)
    }
    fn pool(&mut self) -> Vec<Sid> {
        self.fake.pool()
    }
    fn install_id(&mut self) -> Result<Option<String>, String> {
        self.fake.install_id()
    }
    fn settle(
        &mut self,
        id: &str,
        pool_user: Option<&Sid>,
        install: Option<&str>,
        state: MarkerState,
    ) -> io::Result<()> {
        self.fake.settle(id, pool_user, install, state)
    }
    fn remove(&mut self, id: &str) -> io::Result<()> {
        assert!(!self.alive, "{id} went clean with a pool process alive");
        self.fake.remove(id)
    }
    /// The stop reserves `StopNext`, kills every pool process and completes.
    fn request_stop(&mut self) -> bool {
        self.fake.request_stop();
        self.alive = false;
        self.fake.done.stop = self.fake.stop_next;
        self.fake.stop_next += 1;
        true
    }
    /// The sweep reserves `SweepNext`; the trigger fires before it deletes the task, and the
    /// sweep stops the task's instance but not the process it started; then it completes.
    fn request_sweep(&mut self) -> Result<(), String> {
        self.fake.request_sweep()?;
        if self.trigger {
            self.trigger = false;
            self.alive = true;
        }
        self.fake.done.sweep = self.fake.sweep_next;
        self.fake.sweep_next += 1;
        Ok(())
    }
    fn pause(&mut self, pause: Duration) {
        self.fake.pause(pause);
    }
    fn note(&mut self, line: &str) {
        self.fake.note(line);
    }
    fn outer(&mut self, pid: u32) -> OuterSeen {
        self.fake.outer(pid)
    }
    fn task_repair(&mut self) -> Option<&'static str> {
        self.fake.task_repair()
    }
    fn heal_tasks(&mut self) -> bool {
        self.fake.heal_tasks()
    }
    fn admin_copy_stamp(&mut self) -> Option<String> {
        self.fake.admin_copy_stamp()
    }
}

#[test]
fn a_trigger_that_respawns_a_pool_process_keeps_the_marker_until_the_final_stop() {
    let mut world = World::new(5);
    world.request_stop();
    // F4: the first stop completed. The re-mark requests the sweep, during which the trigger
    // starts a process; the sweep completes, but the marker isn't clean: it awaits the final
    // stop, whose number is read only now, and the logon is refused.
    let refused = logon_check(&mut world, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert!(refused.why.contains("final stop"), "{refused:?}");
    assert!(world.fake.has("a1"));
    assert!(!world.alive, "the final stop F4 requested killed it");
    // The final stop completed: the next logon goes, and the marker is gone.
    assert_eq!(logon_check(&mut world, &sid(POOL_A)), Ok(()));
    assert!(!world.fake.has("a1"));
    let order: Vec<&str> = world
        .fake
        .log
        .iter()
        .map(String::as_str)
        .filter(|line| ["stop", "sweep"].contains(line))
        .collect();
    assert_eq!(order, ["stop", "sweep", "stop"]);
}

#[test]
fn neither_a_launch_nor_the_desktops_start_clears_a_marker_before_its_final_stop() {
    let mut world = World::new(5);
    world.request_stop();
    // A launch re-marks it and the sweep runs; the respawned process lives, the marker stays.
    assert_eq!(await_sweeps(&mut world), Ok(()));
    assert!(world.alive);
    let swept = MarkerState::AwaitingSweepBeforeStop { sweep: 9 };
    assert_eq!(world.fake.state("a1"), Some(swept));
    assert_eq!(await_sweeps(&mut world), Ok(()), "it is F4's now");
    assert!(world.fake.has("a1"));
    // The desktop's start reads the final stop's number and requests that stop.
    let found = start_up(&mut world).unwrap();
    assert!(found.stop_requested);
    assert_eq!(found.left, 1);
    let last = MarkerState::AwaitingFinalStop { stop: 6 };
    assert_eq!(world.fake.state("a1"), Some(last));
    assert!(!world.alive);
    // With that stop completed, whoever looks next clears it.
    assert_eq!(start_up(&mut world).unwrap(), StartUp::default());
    assert!(!world.fake.has("a1"));
}

#[test]
fn a_swept_marker_whose_final_stop_cant_be_read_refuses_the_logon_without_a_stop() {
    let mut world = World::new(5);
    world.request_stop();
    world.trigger = false;
    world.stop_next_fails = true;
    world.fake.log.clear();
    let refused = logon_check(&mut world, &sid(POOL_A)).unwrap_err();
    assert_eq!(
        refused.exit,
        RunExit::AwaitingSweep,
        "no stop number to wait for"
    );
    assert!(refused.why.contains("completed sweep"), "{refused:?}");
    let swept = MarkerState::AwaitingSweepBeforeStop { sweep: 9 };
    assert_eq!(world.fake.state("a1"), Some(swept));
    assert_eq!(world.fake.asked("stop"), 0);
    // Once the number reads, the next logon re-marks it, requests the stop and is refused.
    world.stop_next_fails = false;
    let refused = logon_check(&mut world, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert_eq!(world.fake.asked("sweep"), 1, "its sweep had completed");
    assert_eq!(logon_check(&mut world, &sid(POOL_A)), Ok(()));
}

#[test]
fn a_final_stop_still_running_refuses_the_logon_and_asks_for_it_again() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(FINAL));
    fake.done = Sequences { stop: 4, sweep: 9 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert_eq!(fake.state("a1"), Some(FINAL));
    assert_eq!(
        fake.log,
        ["stop"],
        "no number read: the stop is already awaited"
    );
    // Another pool user's final stop doesn't hold this one's logon.
    assert_eq!(logon_check(&mut fake, &sid(POOL_B)), Ok(()));
}
