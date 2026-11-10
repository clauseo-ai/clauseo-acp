//! Reading the job's pid list through a fake query: a list that outgrows its buffer is asked
//! again from its latest count, a failed call is retried, and one that keeps failing is an error
//! with its Win32 code, never an empty list.

use std::io;

use super::{Asked, JOB_PIDS_FIRST, JOB_PIDS_TRIES, MORE_DATA, job_pids_from};

/// What Windows writes for a job of `pids` read into `room` pids: the list cut at the buffer's
/// end, with `ERROR_MORE_DATA`, when they don't fit.
fn windows(pids: &[usize], room: usize) -> Asked {
    let fits = pids.len() <= room;
    Asked {
        result: if fits {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(MORE_DATA))
        },
        assigned: pids.len(),
        listed: pids.iter().take(room).copied().collect(),
    }
}

fn job(count: usize) -> Vec<usize> {
    (1..=count).map(|n| n * 4).collect()
}

#[test]
fn a_job_with_more_pids_than_the_first_buffer_is_read_again_from_its_count() {
    let pids = job(300);
    let mut rooms = Vec::new();
    let got = job_pids_from(|room| {
        rooms.push(room);
        windows(&pids, room)
    });
    let want: Vec<u32> = pids
        .iter()
        .map(|pid| u32::try_from(*pid).unwrap())
        .collect();
    assert_eq!(got.unwrap(), want);
    // The second buffer is sized from the count the first call gave, plus headroom.
    assert_eq!(rooms, vec![JOB_PIDS_FIRST, 364]);
}

#[test]
fn a_list_that_grows_between_calls_is_read_again_each_time() {
    // A fork bomb at the cap: each call finds more processes than the last buffer was sized for.
    let mut calls = 0usize;
    let mut rooms = Vec::new();
    let got = job_pids_from(|room| {
        calls += 1;
        rooms.push(room);
        let now = match calls {
            1 => 100,
            2 => 200,
            _ => 257,
        };
        windows(&job(now), room)
    });
    assert_eq!(got.unwrap().len(), 257);
    assert_eq!(rooms, vec![JOB_PIDS_FIRST, 164, 328]);
}

#[test]
fn a_short_list_without_an_error_is_read_again() {
    // More processes assigned than pids listed, though the call succeeded: never taken as whole.
    let mut calls = 0usize;
    let got = job_pids_from(|room| {
        calls += 1;
        let mut asked = windows(&job(10), room);
        if calls == 1 {
            asked.assigned = 11;
        }
        asked
    });
    assert_eq!(got.unwrap().len(), 10);
    assert_eq!(calls, 2);
}

#[test]
fn a_failed_first_call_then_a_good_one_gives_the_list() {
    let mut calls = 0usize;
    let got = job_pids_from(|room| {
        calls += 1;
        if calls == 1 {
            return Asked {
                result: Err(io::Error::from_raw_os_error(24)),
                assigned: 0,
                listed: Vec::new(),
            };
        }
        windows(&job(3), room)
    });
    assert_eq!(got.unwrap(), vec![4, 8, 12]);
    assert_eq!(calls, 2);
}

#[test]
fn a_list_that_keeps_failing_is_an_error_with_its_code_never_an_empty_list() {
    let mut calls = 0usize;
    let got = job_pids_from(|_| {
        calls += 1;
        Asked {
            result: Err(io::Error::from_raw_os_error(31)),
            assigned: 0,
            listed: Vec::new(),
        }
    });
    assert_eq!(got.unwrap_err().raw_os_error(), Some(31));
    assert_eq!(calls, JOB_PIDS_TRIES);
    // A list that stays short is ERROR_MORE_DATA, the number the sweep's log line gives.
    let got = job_pids_from(|room| {
        let mut asked = windows(&job(5), room);
        asked.assigned = 6;
        asked
    });
    assert_eq!(got.unwrap_err().raw_os_error(), Some(MORE_DATA));
}
