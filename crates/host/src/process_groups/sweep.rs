//! The start-up sweep of the process-group records (Unix). See `process_groups.rs`.

use std::io::Read as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::time::Duration;

use clauseo_process::{
    Member, Presence, StartStamp, boot_stamp, group_members, kill_group, kill_member, presence,
};
use serde::Deserialize;

use crate::clock::Clock;
use crate::log_file::LIFECYCLE;

/// The most the sweep takes in all, on the host's clock; the records it did not reach stay for
/// the next start.
const BUDGET: Duration = Duration::from_secs(2);
/// How long one ended group gets to empty.
const EMPTY_WAIT: Duration = Duration::from_millis(500);
/// How often an ended group is listed again meanwhile.
const POLL: Duration = Duration::from_millis(25);
/// The record format's version; a record of any other, or without a field of this one (an
/// earlier build's, with no `sid`), is unusable and deleted unswept.
pub(super) const VERSION: u32 = 2;
/// The most bytes a record may hold; a real one is about 120.
const MAX_RECORD: u64 = 4096;

/// A record as written (`Recorder::write`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    v: u32,
    pgid: i32,
    leader_start: String,
    sid: u32,
    boot: String,
}

/// A record read and checked: its group (above 1), its leader's start and session.
#[derive(Debug)]
struct Checked {
    pgid: u32,
    leader_start: StartStamp,
    sid: u32,
    boot: String,
}

/// What the sweep did, for its one log line.
#[derive(Debug, Default)]
struct Tally {
    /// Records of this boot whose group was checked.
    found: usize,
    /// Groups signalled.
    ended: usize,
    /// Records whose group is provably not the recorded one: its id leads another process, or a
    /// member is in another session or older than the leader.
    reused: usize,
    /// Records whose group could not be proved either way (a leader there but unreadable, a
    /// listing that failed, a recorded session that is the group's own): left alone.
    unsure: usize,
    /// Members still running after a group's wait.
    left: usize,
    /// Records refused unread (a link, another link, another owner, too big, unparsable).
    refused: usize,
    /// Records of another boot.
    other_boot: usize,
    /// Records not reached within the budget, kept for the next start.
    not_reached: usize,
}

/// The sweep's deadline on the host's clock (`Clock::now().mono`). The waits themselves are
/// `recv_timeout`s; with a `ManualClock` the deadline never passes, and each group's wait is
/// still bounded by its poll count.
struct Deadline<'a> {
    clock: &'a dyn Clock,
    at: Duration,
}

impl Deadline<'_> {
    /// Whether the deadline has passed.
    fn passed(&self) -> bool {
        self.clock.now().mono >= self.at
    }

    /// Waits [`POLL`] (less when the deadline is nearer); false once it has passed.
    fn pause(&self) -> bool {
        let left = self.at.saturating_sub(self.clock.now().mono);
        let wait = POLL.min(left);
        if wait.is_zero() {
            return false;
        }
        let (_keep, never) = std::sync::mpsc::channel::<()>();
        let _timed_out = never.recv_timeout(wait);
        true
    }
}

/// Sweeps the records in `folder`, and answers this boot's stamp (`None` when it cannot be
/// read: then no record is acted on, and all stay).
pub(super) fn sweep(folder: &Path, clock: &dyn Clock) -> Option<String> {
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
    let deadline = Deadline {
        clock,
        at: clock.now().mono.saturating_add(BUDGET),
    };
    for entry in entries.flatten() {
        if deadline.passed() {
            tally.not_reached += 1;
            continue;
        }
        let path = entry.path();
        match read(&path) {
            None => tally.refused += 1,
            Some(record) if record.boot != now => tally.other_boot += 1,
            Some(record) => {
                tally.found += 1;
                end(&record, &mut tally, &deadline);
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
            unsure = tally.unsure,
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
fn end(record: &Checked, tally: &mut Tally, deadline: &Deadline<'_>) {
    let pgid = record.pgid;
    match presence(pgid) {
        Presence::There(Some(start)) if start == record.leader_start => {
            // The agent itself (running, or a zombie): its group, and the agent too in case it
            // left the group.
            kill_group(pgid);
            kill_member(&Member {
                pid: pgid,
                start,
                session: record.sid,
            });
            tally.ended += 1;
            tally.left += wait_empty(pgid, record.leader_start, deadline);
        }
        Presence::There(Some(_)) => tally.reused += 1,
        // A process holds the id but its start is unreadable (another user's): unprovable.
        Presence::There(None) => tally.unsure += 1,
        Presence::Gone => match leaderless(record) {
            Leaderless::Empty => {}
            Leaderless::Ours => {
                kill_group(pgid);
                tally.ended += 1;
                tally.left += wait_empty(pgid, record.leader_start, deadline);
            }
            Leaderless::Reused => tally.reused += 1,
            Leaderless::Unsure => tally.unsure += 1,
        },
    }
}

/// What a recorded group with no leader is.
enum Leaderless {
    /// No member runs.
    Empty,
    /// Provably the agent's: every member is in the agent's recorded session (which is not the
    /// group's own, so no member made a session of it with `setsid`) and none started before
    /// the agent.
    Ours,
    /// Provably another: a member is in another session, or started before the agent.
    Reused,
    /// The listing failed or may be cut short, or the recorded session is the group's id.
    Unsure,
}

/// Reads `record`'s leader-less group. An id is not given again while a member of its group
/// lives, but once the agent's group emptied a later process may take the id, `setsid` or
/// `setpgid(0, 0)`, and exit, leaving a group of its own children. The agent never left the
/// host's session (`process_group(0)`, not `setsid`); a `setsid` daemon's group is in a session
/// of its own, and other groups often in another (a terminal's) or with members older than the
/// agent. A later group in the host's own session with only newer members is not told apart: a
/// residual (`process_groups.rs`).
fn leaderless(record: &Checked) -> Leaderless {
    let Some(members) = group_members(record.pgid) else {
        return Leaderless::Unsure;
    };
    if members.is_empty() {
        return Leaderless::Empty;
    }
    if record.sid == record.pgid {
        return Leaderless::Unsure;
    }
    let foreign = members
        .iter()
        .any(|member| member.session != record.sid || member.start < record.leader_start);
    if foreign {
        Leaderless::Reused
    } else {
        Leaderless::Ours
    }
}

/// Waits up to [`EMPTY_WAIT`] (and within `deadline`) for group `pgid` to hold no running
/// member that started at or after `leader_start`; answers how many still do.
fn wait_empty(pgid: u32, leader_start: StartStamp, deadline: &Deadline<'_>) -> usize {
    let mut waited = Duration::ZERO;
    loop {
        let left = group_members(pgid).map_or(0, |members| {
            members.iter().filter(|m| m.start >= leader_start).count()
        });
        if left == 0 || waited >= EMPTY_WAIT || !deadline.pause() {
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
        (VERSION, Some(pgid), Some(leader_start)) => Some(Checked {
            pgid,
            leader_start,
            sid: record.sid,
            boot: record.boot,
        }),
        _ => refuse("format"),
    }
}
