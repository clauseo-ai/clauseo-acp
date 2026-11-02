//! Cells: the cap, `time_open`, `onboarding_step`, the UTC day and the 14-day and 4-week limits.

use clauseo_counts::schema::{
    Count, Crash, CrashKind, CrashProcess, Event, Onboarding, OnboardingStep, TimeOpenBucket,
};

use crate::support::*;

#[test]
fn a_cell_stops_at_9999() {
    let scratch = Scratch::new("cap");
    let counts = saving(&scratch);
    counts.tick(at(2, 0)).unwrap();
    for _ in 0..10_050 {
        counts.bump(COLD);
    }
    let cells = counts.today();
    assert_eq!(cells.app_opened.len(), 1);
    assert_eq!(cells.app_opened[0].n, Count::new(9999).unwrap());
}

#[test]
fn time_open_is_one_cell_a_day_split_at_midnight() {
    let scratch = Scratch::new("time-open");
    let counts = saving(&scratch);
    // Day 2: in the foreground 20:00 to 23:00, ticking every 60 s: 3 hours.
    counts.set_foreground(true, at(2, 20 * HOUR));
    let mut now = at(2, 20 * HOUR);
    while now < at(2, 23 * HOUR) {
        now = now.plus(60);
        counts.tick(now).unwrap();
    }
    // Asleep from 23:00 to 23:59: a gap that long is not counted.
    counts.tick(at(2, 23 * HOUR + 59 * 60)).unwrap();
    // 60 s before midnight go to day 2, 30 s after it to day 3, and 30 s more until it leaves.
    counts.tick(at(3, 30)).unwrap();
    counts.set_foreground(false, at(3, 60));
    let queued = counts.queued_days();
    assert_eq!(queued, vec![day(2)]);

    // Day 3 closes with its 60 s.
    counts.tick(at(4, 0)).unwrap();
    let reports = read_queue(&scratch);
    let day2 = &reports[0];
    assert_eq!(day2.cells.time_open.len(), 1);
    assert_eq!(day2.cells.time_open[0].key.bucket, TimeOpenBucket::Lt6h);
    assert_eq!(day2.cells.time_open[0].n, Count::ONE);
    let day3 = &reports[1];
    assert_eq!(day3.cells.time_open.len(), 1);
    assert_eq!(day3.cells.time_open[0].key.bucket, TimeOpenBucket::Lt15m);
}

fn read_queue(scratch: &Scratch) -> Vec<clauseo_counts::schema::DayReport> {
    let dir = counts_dir(scratch).join("queue");
    files(&dir)
        .iter()
        .map(|name| serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap())
        .collect()
}

#[test]
fn onboarding_step_counts_once_per_step_per_install() {
    let scratch = Scratch::new("onboarding");
    let counts = saving(&scratch);
    let step = Event::Onboarding(Onboarding {
        step: OnboardingStep::FolderChosen,
    });
    counts.tick(at(2, 0)).unwrap();
    counts.bump(step);
    counts.bump(step);
    assert_eq!(counts.today().onboarding_step.len(), 1);
    assert_eq!(counts.today().onboarding_step[0].n, Count::ONE);

    // A new day, and a reopen: still counted.
    counts.tick(at(3, 0)).unwrap();
    let counts = saving(&scratch);
    counts.tick(at(3, 60)).unwrap();
    counts.bump(step);
    assert_eq!(counts.today().onboarding_step.len(), 0);

    // A new install (off, then on) counts it once more.
    counts.set_enabled(false).unwrap();
    counts.set_enabled(true).unwrap();
    counts.tick(at(3, 120)).unwrap();
    counts.bump(step);
    assert_eq!(counts.today().onboarding_step[0].n, Count::ONE);
}

#[test]
fn the_day_closes_at_utc_midnight() {
    let scratch = Scratch::new("midnight");
    let counts = saving(&scratch);
    counts.tick(at(2, DAY - 1)).unwrap();
    counts.bump(COLD);
    assert_eq!(counts.queued_days().len(), 0);
    counts.tick(at(3, 0)).unwrap();
    assert_eq!(counts.queued_days(), vec![day(2)]);
    counts.bump(COLD);
    counts.bump(COLD);
    assert_eq!(counts.today().app_opened[0].n.get(), 2);
    let report = &read_queue(&scratch)[0];
    assert_eq!(report.day, day(2));
    assert_eq!(report.cells.app_opened[0].n, Count::ONE);
}

#[test]
fn a_fifteenth_closed_day_pushes_out_the_oldest() {
    let scratch = Scratch::new("fifteen");
    // Before the first act nothing is sent, so the queue only grows.
    let counts = clauseo_counts::Counts::open(scratch.path(), about(), true, false, SEED);
    fill_days(&counts, 2, 15);
    assert_eq!(counts.queued_days().len(), 14);
    assert_eq!(counts.queued_days()[0], day(2));
    counts.tick(at(16, 12 * HOUR)).unwrap();
    counts.bump(COLD);
    counts.tick(at(17, 0)).unwrap();
    let queued = counts.queued_days();
    assert_eq!(queued.len(), 14);
    assert_eq!(queued[0], day(3));
    assert_eq!(queued[13], day(16));
}

#[test]
fn a_tally_older_than_4_weeks_is_dropped() {
    let scratch = Scratch::new("tally-age");
    let counts = saving(&scratch);
    counts.tick(at(2, 0)).unwrap();
    counts.bump(Event::Crash(Crash {
        process: CrashProcess::Host,
        kind: CrashKind::Panic,
        after_launch: true,
    }));
    counts.tick(at(2, 60)).unwrap();
    let tally = counts_dir(&scratch).join("tally");
    assert_eq!(files(&tally), vec!["2026-W45.json".to_owned()]);
    // Never sent (no `send_due`); four weeks on it is still kept, the fifth drops it.
    counts.tick(at(2 + 28, 60)).unwrap();
    assert!(files(&tally).contains(&"2026-W45.json".to_owned()));
    counts.tick(at(2 + 35, 60)).unwrap();
    assert!(!files(&tally).contains(&"2026-W45.json".to_owned()));
}
