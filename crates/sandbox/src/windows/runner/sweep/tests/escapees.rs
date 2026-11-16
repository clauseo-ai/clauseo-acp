//! The escapees any sweep reports, and a sweep that can't read its own start time, through the
//! same fake.

use super::*;

#[test]
fn a_sweep_that_cant_read_its_own_start_kills_nothing() {
    struct NoClock(Fake);
    impl Processes for NoClock {
        type Process = u32;
        fn list(&mut self) -> io::Result<Vec<Seen>> {
            self.0.list()
        }
        fn named(&mut self) -> io::Result<Vec<Seen>> {
            self.0.named()
        }
        fn open(&mut self, pid: u32) -> io::Result<Option<u32>> {
            self.0.open(pid)
        }
        fn job_pids(&mut self) -> io::Result<Vec<u32>> {
            self.0.job_pids()
        }
        fn started(&mut self, process: &u32) -> io::Result<u64> {
            self.0.started(process)
        }
        fn own_start(&mut self) -> io::Result<u64> {
            Err(io::Error::from_raw_os_error(6))
        }
        fn kill(&mut self, process: &u32) -> io::Result<()> {
            self.0.kill(process)
        }
        fn exited(&mut self, process: &u32) -> bool {
            self.0.exited(process)
        }
        fn pause(&mut self, pause: Duration) {
            self.0.pause(pause);
        }
    }
    let mut fake = machine();
    fake.procs.insert(31, pool_proc("task.exe"));
    let mut calls = NoClock(fake);
    let got = sweep(&mut calls, &sid(POOL), &[30], Point::Tick, &mut |_| {});
    assert_eq!(
        got,
        Err(SweepError::OwnStart {
            code: "6".to_owned()
        })
    );
    assert_eq!(calls.0.killed, Vec::<u32>::new());
    // Not an escapee: no process of the pool user's was found unkillable.
    assert_eq!(got.unwrap_err().escapees(), None);
}

#[test]
fn a_process_that_cant_be_opened_or_ended_is_an_escapee_and_one_that_cant_be_checked_isnt() {
    let one = |image: &str| {
        Some(Escapees {
            count: 1,
            images: vec![image.to_owned()],
        })
    };
    let (pid, code) = (12, "5".to_owned());
    let image = "hidden.exe".to_owned();
    let open = SweepError::Open {
        pid,
        image: image.clone(),
        code: code.clone(),
    };
    let kill = SweepError::Kill {
        pid,
        image: image.clone(),
        code: code.clone(),
    };
    assert_eq!(open.escapees(), one("hidden.exe"));
    assert_eq!(kill.escapees(), one("hidden.exe"));
    let job = SweepError::JobList(code.clone());
    let started = SweepError::Started { pid, image, code };
    for plain in [
        job,
        started,
        SweepError::List("5".to_owned()),
        SweepError::StillRunning,
        SweepError::Left {
            count: 2,
            unkillable: 0,
            images: Vec::new(),
        },
    ] {
        assert_eq!(plain.escapees(), None, "{plain:?}");
    }
    // The first act fails at the first one.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("hidden.exe"));
    fake.deny_open.insert(12);
    let got = run(&mut fake, &[], Point::Start).0.unwrap_err();
    assert_eq!(got.escapees(), one("hidden.exe"));
}

#[test]
fn a_later_sweep_counts_its_escapees_and_names_each_image_once_up_to_the_cap() {
    for point in [Point::Tick, Point::End] {
        let mut fake = machine();
        // Two hidden copies of one image, nine others, and one that only can't be checked.
        for pid in 20..22 {
            fake.procs.insert(
                pid,
                Proc {
                    hidden: true,
                    ..pool_proc("same.exe")
                },
            );
        }
        let images = [
            "a.exe", "b.exe", "c.exe", "d.exe", "e.exe", "f.exe", "g.exe", "h.exe", "i.exe",
        ];
        for (pid, image) in (30..).zip(images) {
            fake.procs.insert(pid, pool_proc(image));
            fake.deny_kill.insert(pid, u32::MAX);
        }
        let got = run(&mut fake, &[], point).0.unwrap_err();
        let escapees = got.escapees().unwrap();
        assert_eq!(escapees.count, 11, "{point}");
        assert_eq!(escapees.images.len(), IMAGES_CAP, "{point}");
        assert_eq!(escapees.images[..2], ["same.exe", "a.exe"], "{point}");
    }
}
