//! Linux: Chrome's children from `/proc` (`postcheck.rs`). The tree from each process's parent
//! pid in its `stat`, read once per process: a look lists `/proc` and reads the `stat` of the
//! pids it has not seen before, and keeps the rest (a pid is not reused while it is listed, and
//! the kernel hands pids out in turn, so one does not leave and come back between two looks a
//! quarter second apart). Each child's start time and state from its `stat` (a zombie is gone);
//! its arguments from `cmdline`, its seccomp mode from `status` and its namespaces from `ns/pid`
//! and `ns/user`. Plain file reads; what they mean is in `judge.rs`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;

use super::judge::{self, Namespaces};
use super::{Source, descendants};

/// Reads `/proc`, remembering each listed process's parent, and each child's `stat` as the last
/// listing read it (for [`Source::stamp`] in the same look).
#[derive(Default)]
pub(super) struct Procfs {
    parents: BTreeMap<u32, u32>,
    listed: BTreeMap<u32, Stat>,
}

impl Procfs {
    pub(super) fn new() -> Procfs {
        Procfs::default()
    }
}

impl Source for Procfs {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        let mut listed = Vec::new();
        for entry in fs::read_dir("/proc").ok()?.flatten() {
            if let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            {
                listed.push(pid);
            }
        }
        listed.sort_unstable();
        self.parents
            .retain(|pid, _| listed.binary_search(pid).is_ok());
        for &pid in &listed {
            if !self.parents.contains_key(&pid)
                && let Some(stat) = stat_of(pid)
            {
                self.parents.insert(pid, stat.parent);
            }
        }
        let pairs: Vec<(u32, u32)> = self
            .parents
            .iter()
            .map(|(&pid, &parent)| (pid, parent))
            .collect();
        // Gone since the listing, or a zombie: not a child to judge.
        self.listed = descendants(chrome, &pairs)
            .into_iter()
            .filter_map(|pid| Some((pid, stat_of(pid)?)))
            .filter(|(_, stat)| !stat.zombie)
            .collect();
        Some(self.listed.keys().copied().collect())
    }

    fn stamp(&mut self, pid: u32) -> Option<u64> {
        match self.listed.get(&pid) {
            Some(stat) => Some(stat.started),
            None => stat_of(pid)
                .filter(|stat| !stat.zombie)
                .map(|stat| stat.started),
        }
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        fs::read(format!("/proc/{pid}/cmdline")).ok()
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok();
        judge::linux(status.as_deref(), &namespaces(chrome), &namespaces(pid))
    }
}

/// What a process's `stat` says.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Stat {
    pub(super) parent: u32,
    /// Its start time, in clock ticks after boot.
    pub(super) started: u64,
    /// State `Z` or `X`: exited, not yet reaped.
    pub(super) zombie: bool,
}

/// `pid`'s `stat`, read with one call: the line is far shorter than the buffer, and a `/proc`
/// file reports no size, so `read_to_string` would read it in several small steps.
fn stat_of(pid: u32) -> Option<Stat> {
    let mut buffer = [0_u8; 1024];
    let read = fs::File::open(format!("/proc/{pid}/stat"))
        .and_then(|mut file| file.read(&mut buffer))
        .ok()?;
    parse_stat(std::str::from_utf8(buffer.get(..read)?).ok()?)
}

/// A `/proc/<pid>/stat` line. The command name is in parentheses and may hold spaces and
/// parentheses itself, so the fields are read after the last `)`: the state (field 3), the
/// parent pid (4) and the start time (22).
pub(super) fn parse_stat(stat: &str) -> Option<Stat> {
    let (_, rest) = stat.rsplit_once(')')?;
    let fields: Vec<&str> = rest.split_ascii_whitespace().collect();
    Some(Stat {
        parent: fields.get(1)?.parse().ok()?,
        started: fields.get(19)?.parse().ok()?,
        zombie: matches!(*fields.first()?, "Z" | "X"),
    })
}

/// `pid`'s PID and user namespaces.
fn namespaces(pid: u32) -> Namespaces {
    let link = |name: &str| {
        fs::read_link(format!("/proc/{pid}/ns/{name}"))
            .ok()
            .map(|target| target.to_string_lossy().into_owned())
    };
    Namespaces {
        pid: link("pid"),
        user: link("user"),
    }
}
