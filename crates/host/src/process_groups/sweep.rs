//! The start-up sweep of the process-group records (Unix). See `process_groups.rs`.

use std::io::Read as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::Duration;

use clauseo_process::{Member, StartStamp, boot_stamp, group_members, kill_group, kill_member};
use serde::Deserialize;

use crate::log_file::LIFECYCLE;

/// The most the sweep waits in all; the records it did not reach stay for the next start.
const BUDGET: Duration = Duration::from_secs(2);
/// How long one ended group gets to empty.
const EMPTY_WAIT: Duration = Duration::from_millis(500);
/// How often an ended group is listed again meanwhile.
const POLL: Duration = Duration::from_millis(25);
/// The most bytes a record may hold; a real one is about 120.
const MAX_RECORD: u64 = 4096;

/// A record as written (`Recorder::write`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    v: u32,
    pgid: i32,
    leader_start: String,
    boot: String,
}

/// A record read and checked: its group (above 1) and its leader's start.
#[derive(Debug)]
struct Checked {
    pgid: u32,
    leader_start: StartStamp,
    boot: String,
}

/// What the sweep did, for its one log line.
#[derive(Debug, Default)]
struct Tally {
    /// Records of this boot whose group was checked.
    found: usize,
    /// Groups signalled.
    ended: usize,
    /// Records whose group id now leads another process.
    reused: usize,
    /// Members still running after a group's wait, and members spared as older than the leader.
    left: usize,
    /// Records refused unread (a link, another link, another owner, too big, unparsable).
    refused: usize,
    /// Records of another boot.
    other_boot: usize,
    /// Records not reached within the budget, kept for the next start.
    not_reached: usize,
}

/// The time the sweep may still wait.
struct Budget(Duration);

impl Budget {
    /// Waits `wait` (less when the budget has less); false once nothing is left.
    fn pause(&mut self, wait: Duration) -> bool {
        let wait = wait.min(self.0);
        if wait.is_zero() {
            return false;
        }
        let (_keep, never) = std::sync::mpsc::channel::<()>();
        let _timed_out = never.recv_timeout(wait);
        self.0 = self.0.saturating_sub(wait);
        true
    }

    fn spent(&self) -> bool {
        self.0.is_zero()
    }
}

/// Sweeps the records in `folder`, and answers this boot's stamp (`None` when it cannot be
/// read: then no record is acted on, and all stay).
pub(super) fn sweep(folder: &Path) -> Option<String> {
    let boot = boot_stamp();
    match std::fs::symlink_metadata(folder) {
        Err(_) => return boot,
        Ok(meta) if !meta.is_dir() => {
            tracing::warn!("the process-group records folder is not a real folder; it is left");
            return boot;
        }
        Ok(_) => {}
    }
    let Some(now) = boot else {
        tracing::warn!("this boot's stamp was not read; the process-group records stay unchecked");
        return None;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        tracing::warn!("the process-group records folder was not read");
        return Some(now);
    };
    let mut tally = Tally::default();
    let mut budget = Budget(BUDGET);
    for entry in entries.flatten() {
        if budget.spent() {
            tally.not_reached += 1;
            continue;
        }
        let path = entry.path();
        match read(&path) {
            None => tally.refused += 1,
            Some(record) if record.boot != now => tally.other_boot += 1,
            Some(record) => {
                tally.found += 1;
                end(&record, &mut tally, &mut budget);
            }
        }
        super::remove(&path);
    }
    if tally.found + tally.refused + tally.other_boot + tally.not_reached > 0 {
        tracing::info!(
            target: LIFECYCLE,
            found = tally.found,
            ended = tally.ended,
            reused = tally.reused,
            left = tally.left,
            refused = tally.refused,
            other_boot = tally.other_boot,
            not_reached = tally.not_reached,
            "the agents' process groups an earlier run left were swept"
        );
    }
    Some(now)
}

/// Ends `record`'s group if it is provably the recorded one (`process_groups.rs`'s rule).
fn end(record: &Checked, tally: &mut Tally, budget: &mut Budget) {
    let pgid = record.pgid;
    match clauseo_process::start_stamp(pgid) {
        Some(start) if start == record.leader_start => {
            // The agent itself: its group, and the agent too in case it left the group.
            kill_group(pgid);
            kill_member(&Member { pid: pgid, start });
            tally.ended += 1;
            tally.left += wait_empty(pgid, record.leader_start, budget);
        }
        Some(_) => tally.reused += 1,
        None => {
            let Some(members) = group_members(pgid) else {
                tracing::warn!("a recorded group's members were not listed; it is left");
                return;
            };
            if members.is_empty() {
                return;
            }
            let earlier = members
                .iter()
                .filter(|member| member.start < record.leader_start)
                .count();
            if earlier == 0 {
                kill_group(pgid);
            } else {
                // Not the agent's descendants: the group is not signalled whole.
                tracing::warn!(
                    earlier,
                    "a recorded group holds processes older than its leader; they are spared"
                );
                for member in members.iter().filter(|m| m.start >= record.leader_start) {
                    kill_member(member);
                }
                tally.left += earlier;
            }
            tally.ended += 1;
            tally.left += wait_empty(pgid, record.leader_start, budget);
        }
    }
}

/// Waits up to [`EMPTY_WAIT`] (within `budget`) for group `pgid` to hold no running member that
/// started at or after `leader_start`; answers how many still do.
fn wait_empty(pgid: u32, leader_start: StartStamp, budget: &mut Budget) -> usize {
    let mut waited = Duration::ZERO;
    loop {
        let left = group_members(pgid).map_or(0, |members| {
            members.iter().filter(|m| m.start >= leader_start).count()
        });
        if left == 0 || waited >= EMPTY_WAIT || !budget.pause(POLL) {
            if left > 0 {
                tracing::warn!(left, "an ended group did not empty in time");
            }
            return left;
        }
        waited += POLL;
    }
}

/// Reads and checks the record at `path`; `None` (with the reason's kind logged) when it is
/// refused: a link, another link to the same file, another user's, not a file, too big, or not
/// a record. The file is opened only after its `lstat`, and its `fstat` must be the same file.
fn read(path: &Path) -> Option<Checked> {
    let refuse = |why: &'static str| {
        tracing::warn!(why, "a process-group record was refused unread and deleted");
        None
    };
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return refuse("unreadable");
    };
    if meta.file_type().is_symlink() {
        return refuse("link");
    }
    if !meta.is_file() {
        return refuse("not a file");
    }
    if meta.nlink() > 1 {
        return refuse("hard link");
    }
    if meta.uid() != nix::unistd::getuid().as_raw() {
        return refuse("owner");
    }
    let Ok(file) = std::fs::File::open(path) else {
        return refuse("unreadable");
    };
    let same = file.metadata().is_ok_and(|opened| {
        opened.dev() == meta.dev()
            && opened.ino() == meta.ino()
            && opened.nlink() == 1
            && opened.is_file()
    });
    if !same {
        return refuse("changed");
    }
    let mut bytes = Vec::new();
    if file.take(MAX_RECORD + 1).read_to_end(&mut bytes).is_err() {
        return refuse("unreadable");
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECORD {
        return refuse("size");
    }
    let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
        return refuse("format");
    };
    let pgid = u32::try_from(record.pgid).ok().filter(|&pgid| pgid > 1);
    let start = StartStamp::decode(&record.leader_start);
    match (record.v, pgid, start) {
        (1, Some(pgid), Some(leader_start)) => Some(Checked {
            pgid,
            leader_start,
            boot: record.boot,
        }),
        _ => refuse("format"),
    }
}
